// Library: "Liked tracks", playlists (string ids, editing your own), albums/artists/podcasts,
// the "Library" screen (items/folderChildren), local pins. Models mirror LibraryEvent;
// Lagged — re-read everything from the service.
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use cxx::UniquePtr;
use cxx_qt::{CxxQtThread, CxxQtType, Threading};
use cxx_qt_lib::{QString, QStringList};
use md3_model::{ListChange, PlayContext, PlaylistId, contexts};
use yt_services::{AuthEvent, HistoryEvent, LibraryEvent, LibraryKind};
use tokio::sync::{Semaphore, broadcast};

use super::{Delivery, forward_events, json_str, model_ptr, string_list, track_from_map, with_model};
use crate::context::ctx;
use crate::models::{DictListModel, ModelKind, Row, apply_list_change, apply_row_change, to_row_change, to_rows};
use crate::qt::{QVariantList, QVariantMap, qvariant_to_json};

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
        include!("cxx-qt-lib/qlist.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qproperty(*mut QObject, playlists, READ = playlists_ptr, CONSTANT)]
        #[qproperty(*mut QObject, liked, READ = liked_ptr, CONSTANT)]
        #[qproperty(*mut QObject, albums, READ = albums_ptr, CONSTANT)]
        #[qproperty(*mut QObject, artists, READ = artists_ptr, CONSTANT)]
        #[qproperty(*mut QObject, shows, READ = shows_ptr, CONSTANT)]
        #[qproperty(*mut QObject, recent, READ = recent_ptr, CONSTANT)]
        #[qproperty(QString, error_text, cxx_name = "errorText", READ, NOTIFY = error_changed)]
        #[qproperty(i32, likes_revision, cxx_name = "likesRevision", READ, NOTIFY = likes_changed)]
        #[qproperty(i32, saved_revision, cxx_name = "savedRevision", READ, NOTIFY = saved_changed)]
        #[qproperty(i32, pins_revision, cxx_name = "pinsRevision", READ, NOTIFY = pins_changed)]
        #[qproperty(i32, pages_loaded, cxx_name = "pagesLoaded", READ, NOTIFY = pages_changed)]
        #[qproperty(i32, pages_total, cxx_name = "pagesTotal", READ, NOTIFY = pages_changed)]
        type Library = super::LibraryRust;

        fn playlists_ptr(self: &Library) -> *mut QObject;
        fn liked_ptr(self: &Library) -> *mut QObject;
        fn albums_ptr(self: &Library) -> *mut QObject;
        fn artists_ptr(self: &Library) -> *mut QObject;
        fn shows_ptr(self: &Library) -> *mut QObject;
        fn recent_ptr(self: &Library) -> *mut QObject;

        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut Library>);
        #[qsignal]
        #[cxx_name = "likesChanged"]
        fn likes_changed(self: Pin<&mut Library>);
        #[qsignal]
        #[cxx_name = "savedChanged"]
        fn saved_changed(self: Pin<&mut Library>);
        #[qsignal]
        #[cxx_name = "pinsChanged"]
        fn pins_changed(self: Pin<&mut Library>);
        #[qsignal]
        #[cxx_name = "pagesChanged"]
        fn pages_changed(self: Pin<&mut Library>);
        /// the tree/sort changed — the items()/folderChildren() models are already updated
        #[qsignal]
        #[cxx_name = "entriesChanged"]
        fn entries_changed(self: Pin<&mut Library>);
        /// the playlist header changed — re-read playlistInfo(id)
        #[qsignal]
        #[cxx_name = "playlistInfoChanged"]
        fn playlist_info_changed(self: Pin<&mut Library>, playlist_id: QString);
        /// A short message (the outcome of an action, an error with a rollback).
        #[qsignal]
        fn notice(self: Pin<&mut Library>, text: QString);

        #[qinvokable]
        fn refresh(self: &Library);

    // --- playlists ---
        /// Playlist track model (cached by id). Opening the screen: the owner count grows, the model
        /// is not recreated; the header is checked against the snapshot — a changed playlist is re-read.
        #[qinvokable]
        #[cxx_name = "playlistTracks"]
        fn playlist_tracks(self: Pin<&mut Library>, playlist_id: &QString) -> *mut QObject;
        /// The screen closed or was hidden: one owner leaves. When no owners are left, an in-flight
        /// track read is cancelled (writes and playback are untouched).
        #[qinvokable]
        #[cxx_name = "releasePlaylist"]
        fn release_playlist(self: Pin<&mut Library>, playlist_id: &QString);
        /// Header: PlaylistItem keys + totalDurationMs, followed; {} — unknown.
        #[qinvokable]
        #[cxx_name = "playlistInfo"]
        fn playlist_info(self: &Library, playlist_id: &QString) -> QMap_QString_QVariant;
        #[qinvokable]
        #[cxx_name = "canEdit"]
        fn can_edit(self: &Library, playlist_id: &QString) -> bool;
        #[qinvokable]
        #[cxx_name = "addToPlaylist"]
        fn add_to_playlist(self: &Library, track: &QMap_QString_QVariant, playlist_id: &QString);
        #[qinvokable]
        #[cxx_name = "addTracks"]
        fn add_tracks(self: &Library, playlist_id: &QString, uris: &QStringList);
        /// Insert at a position ("Undo" after deleting rows).
        #[qinvokable]
        #[cxx_name = "insertTracks"]
        fn insert_tracks(self: &Library, playlist_id: &QString, uris: &QStringList, position: i32);
        /// rows: [{uri, pos, setVideoId}] — deletion needs the id of the occurrence in the playlist
        #[qinvokable]
        #[cxx_name = "removeTracks"]
        fn remove_tracks(self: &Library, playlist_id: &QString, rows: &QList_QVariant);
        #[qinvokable]
        #[cxx_name = "moveTracks"]
        fn move_tracks(self: &Library, playlist_id: &QString, from: i32, to: i32);
        #[qinvokable]
        #[cxx_name = "createPlaylist"]
        fn create_playlist(self: &Library, title: &QString);
        #[qinvokable]
        #[cxx_name = "createPlaylist"]
        fn create_playlist_with(self: &Library, title: &QString, track: &QMap_QString_QVariant);
        #[qinvokable]
        #[cxx_name = "createPlaylistFrom"]
        fn create_playlist_from(self: &Library, title: &QString, uris: &QStringList);
        #[qinvokable]
        #[cxx_name = "renamePlaylist"]
        fn rename_playlist(self: &Library, playlist_id: &QString, title: &QString);
        #[qinvokable]
        #[cxx_name = "setDescription"]
        fn set_description(self: &Library, playlist_id: &QString, text: &QString);
        #[qinvokable]
        #[cxx_name = "setPublic"]
        fn set_public(self: &Library, playlist_id: &QString, value: bool);
        #[qinvokable]
        #[cxx_name = "followPlaylist"]
        fn follow_playlist(self: &Library, playlist_id: &QString);
        #[qinvokable]
        #[cxx_name = "unfollowPlaylist"]
        fn unfollow_playlist(self: &Library, playlist_id: &QString);

    // --- likes, saved ---
        #[qinvokable]
        #[cxx_name = "isLiked"]
        fn is_liked(self: &Library, track_id: &QString) -> bool;
        #[qinvokable]
        #[cxx_name = "toggleLike"]
        fn toggle_like(self: &Library, track: &QMap_QString_QVariant);
        /// Local "do not recommend" + skip if it is playing.
        #[qinvokable]
        fn dislike(self: &Library, track: &QMap_QString_QVariant);
        /// Album/show/episode saved, artist — a subscription.
        #[qinvokable]
        #[cxx_name = "isSaved"]
        fn is_saved(self: &Library, uri: &QString) -> bool;
        #[qinvokable]
        #[cxx_name = "toggleSaved"]
        fn toggle_saved(self: &Library, uri: &QString);

    // --- "Library" screen ---
        /// kind: playlists|albums|artists|shows; owner: all|mine|followed|collab; sort — as in Settings.librarySort.
        #[qinvokable]
        fn items(self: Pin<&mut Library>, kind: &QString, owner: &QString, sort: &QString) -> *mut QObject;
        /// Recently played contexts of all sections (for the side panel), limit — how many.
        #[qinvokable]
        #[cxx_name = "recentEntries"]
        fn recent_entries(self: Pin<&mut Library>, limit: i32) -> *mut QObject;
        #[qinvokable]
        #[cxx_name = "isPinned"]
        fn is_pinned(self: &Library, uri: &QString) -> bool;
        #[qinvokable]
        #[cxx_name = "togglePin"]
        fn toggle_pin(self: &Library, uri: &QString);
    }

    impl cxx_qt::Threading for Library {}
    impl cxx_qt::Initialize for Library {}
}

pub struct LibraryRust {
    playlists: UniquePtr<DictListModel>,
    liked: UniquePtr<DictListModel>,
    albums: UniquePtr<DictListModel>,
    artists: UniquePtr<DictListModel>,
    shows: UniquePtr<DictListModel>,
    /// recently played tracks (HistoryService::recent) — the source for "Recent" in AddTracksPanel
    recent: UniquePtr<DictListModel>,
    playlist_models: HashMap<PlaylistId, UniquePtr<DictListModel>>,
    /// how many screens opened the playlist: the last release cancels an in-flight read (the service decides itself)
    playlist_refs: HashMap<PlaylistId, u32>,
    /// items(kind, owner, sort) → a model; the key is "kind|owner|sort" or "recent|<limit>"
    entry_models: HashMap<String, UniquePtr<DictListModel>>,
    error_text: QString,
    likes_revision: i32,
    saved_revision: i32,
    pins_revision: i32,
    pages_loaded: i32,
    pages_total: i32,
}

impl Default for LibraryRust {
    fn default() -> Self {
        let mut liked = DictListModel::create(ModelKind::Track);
        with_model(&mut liked, |m| m.set_context(PlayContext::new(contexts::LIKED)));
        let mut recent = DictListModel::create(ModelKind::Track);
        with_model(&mut recent, |m| m.set_context(PlayContext::new(contexts::TRACKS)));
        Self {
            playlists: DictListModel::create(ModelKind::Playlist),
            liked,
            albums: DictListModel::create(ModelKind::Album),
            artists: DictListModel::create(ModelKind::Artist),
            shows: DictListModel::create(ModelKind::Show),
            recent,
            playlist_models: HashMap::new(),
            playlist_refs: HashMap::new(),
            entry_models: HashMap::new(),
            error_text: QString::default(),
            likes_revision: 0,
            saved_revision: 0,
            pins_revision: 0,
            pages_loaded: 0,
            pages_total: 0,
        }
    }
}

/// Playlist model context; the name and cover art are for the "Playing from …" card, if the header is already known.
fn playlist_context(id: &PlaylistId) -> PlayContext {
    let context = PlayContext::with_uri(contexts::PLAYLIST, id.uri());
    match ctx().services.library.playlist_info(id) {
        Some(info) => context.titled(info.item.title).with_cover(info.item.cover),
        None => context,
    }
}

/// "Liked tracks" of the signed-in account — the spotify:user:<id>:collection context.
fn liked_context() -> PlayContext {
    match ctx().services.auth.account_id() {
        Some(id) => PlayContext::with_uri(contexts::LIKED, contexts::liked_uri(id.as_str())).titled(mf_i18n::t("Liked Songs")),
        None => PlayContext::new(contexts::LIKED),
    }
}

/// "Library" screen rows are matched by uri (for folders — spotify:folder:<id>): scrolling and delegates are preserved.
const ENTRY_KEY: &str = "uri";

fn pid(id: &QString) -> PlaylistId {
    PlaylistId::from_uri_or_id(&id.to_string())
}

/// A library event already prepared for the model: playlist rows are serialized outside the GUI thread.
/// permit — a back-pressure receipt: it returns to the semaphore after being applied in the GUI.
enum LibraryDelivery {
    /// events were missed — the bridge re-reads the entire service state
    Lagged,
    Ready(LibraryEvent),
    Tracks { id: PlaylistId, change: ListChange<Row>, permit: tokio::sync::OwnedSemaphorePermit },
}

/// How many prepared playlist-track tasks may wait to be applied in the GUI. The receipts (permits)
/// are returned after being applied: if the GUI falls behind, the producer pauses instead of an unbounded queue.
const TRACK_QUEUE: usize = 16;

/// Library events → GUI. Playlist rows (serde) are prepared in a tokio task; scalar events
/// pass through without waiting so the rest is not slowed down. Event order is preserved.
fn forward_library(thread: CxxQtThread<qobject::Library>, mut events: broadcast::Receiver<LibraryEvent>) {
    let gui = Arc::new(Semaphore::new(TRACK_QUEUE));
    ctx().rt.spawn(async move {
        loop {
            let delivery = match events.recv().await {
                Ok(event) => match event {
                    LibraryEvent::PlaylistTracks { id, change } => {
                        // row serialization — before the GUI queue; Semaphore::close is not called, so
                        // a permit is always available
                        let permit = Arc::clone(&gui).acquire_owned().await.expect("semaphore is open");
                        LibraryDelivery::Tracks { id, change: to_row_change(&change), permit }
                    }
                    other => LibraryDelivery::Ready(other),
                },
                Err(broadcast::error::RecvError::Lagged(_)) => LibraryDelivery::Lagged,
                Err(broadcast::error::RecvError::Closed) => break,
            };
            if thread
                .queue(move |object| {
                    object.on_event(delivery);
                    // the permit is released after being applied — that is the GUI receipt
                })
                .is_err()
            {
                break;
            }
        }
    });
}

impl qobject::Library {
    fn playlists_ptr(&self) -> *mut cxx_qt::QObject {
        model_ptr(&self.playlists)
    }

    fn liked_ptr(&self) -> *mut cxx_qt::QObject {
        model_ptr(&self.liked)
    }

    fn recent_ptr(&self) -> *mut cxx_qt::QObject {
        model_ptr(&self.recent)
    }

    fn albums_ptr(&self) -> *mut cxx_qt::QObject {
        model_ptr(&self.albums)
    }

    fn artists_ptr(&self) -> *mut cxx_qt::QObject {
        model_ptr(&self.artists)
    }

    fn shows_ptr(&self) -> *mut cxx_qt::QObject {
        model_ptr(&self.shows)
    }

    fn refresh(&self) {
        ctx().services.library.refresh();
    }

    // --- playlists ---

    fn playlist_tracks(mut self: Pin<&mut Self>, playlist_id: &QString) -> *mut cxx_qt::QObject {
        let id = pid(playlist_id);
        let library = &ctx().services.library;
        let this = self.as_mut().rust_mut().get_mut();
        *this.playlist_refs.entry(id.clone()).or_default() += 1;
        // a stable model instance: we neither recreate nor clear it — changes arrive as events
        let model = this.playlist_models.entry(id.clone()).or_insert_with(|| {
            let mut model = DictListModel::create(ModelKind::Track);
            with_model(&mut model, |m| m.set_context(playlist_context(&id)));
            model
        });
        let ptr = model_ptr(model);
        // the model is empty but the service has ready rows (the bridge was created after the read, or events were missed):
        // re-fill in chunks instead of waiting for a full re-read
        let hydrated = model.as_ref().is_some_and(|m| !m.rows().is_empty());
        if !hydrated {
            library.replay_playlist(&id);
        }
        library.load_playlist(id);
        ptr
    }

    fn release_playlist(mut self: Pin<&mut Self>, playlist_id: &QString) {
        let id = pid(playlist_id);
        let this = self.as_mut().rust_mut().get_mut();
        let Some(count) = this.playlist_refs.get_mut(&id) else { return };
        *count = count.saturating_sub(1);
        if *count == 0 {
            this.playlist_refs.remove(&id);
            ctx().services.library.cancel_playlist(&id);
        }
    }

    fn playlist_info(&self, playlist_id: &QString) -> QVariantMap {
        let Some(info) = ctx().services.library.playlist_info(&pid(playlist_id)) else { return QVariantMap::default() };
        let mut map = match serde_json::to_value(&info.item) {
            Ok(serde_json::Value::Object(map)) => map,
            _ => serde_json::Map::new(),
        };
        map.insert("totalDurationMs".into(), info.total_duration_ms.into());
        map.insert("followed".into(), info.followed.into());
        crate::qt::json_to_qvariantmap(&map)
    }

    fn can_edit(&self, playlist_id: &QString) -> bool {
        ctx().services.library.can_edit(&pid(playlist_id))
    }

    fn add_to_playlist(&self, track: &QVariantMap, playlist_id: &QString) {
        if let Some(track) = track_from_map(track) {
            ctx().services.library.add_to_playlist(track, pid(playlist_id));
        }
    }

    fn add_tracks(&self, playlist_id: &QString, uris: &QStringList) {
        ctx().services.library.add_tracks(pid(playlist_id), string_list(uris));
    }

    fn insert_tracks(&self, playlist_id: &QString, uris: &QStringList, position: i32) {
        let position = usize::try_from(position).unwrap_or(0);
        ctx().services.library.insert_tracks(pid(playlist_id), string_list(uris), position);
    }

    fn remove_tracks(&self, playlist_id: &QString, rows: &QVariantList) {
        let rows = rows
            .iter()
            .filter_map(|row| {
                let row = qvariant_to_json(row);
                let row = row.as_object()?;
                let pos = row.get("pos").and_then(|p| p.as_f64())? as usize;
                Some((json_str(row, "uri"), pos, json_str(row, "setVideoId")))
            })
            .collect();
        ctx().services.library.remove_tracks(pid(playlist_id), rows);
    }

    fn move_tracks(&self, playlist_id: &QString, from: i32, to: i32) {
        if let (Ok(from), Ok(to)) = (usize::try_from(from), usize::try_from(to)) {
            ctx().services.library.move_tracks(pid(playlist_id), from, to);
        }
    }

    fn create_playlist(&self, title: &QString) {
        let title = title.to_string().trim().to_owned();
        if !title.is_empty() {
            ctx().services.library.create_playlist(title, None);
        }
    }

    fn create_playlist_with(&self, title: &QString, track: &QVariantMap) {
        let title = title.to_string().trim().to_owned();
        if !title.is_empty() {
            ctx().services.library.create_playlist(title, track_from_map(track));
        }
    }

    fn create_playlist_from(&self, title: &QString, uris: &QStringList) {
        let title = title.to_string().trim().to_owned();
        if !title.is_empty() {
            ctx().services.library.create_playlist_from(title, string_list(uris));
        }
    }

    fn rename_playlist(&self, playlist_id: &QString, title: &QString) {
        ctx().services.library.rename_playlist(pid(playlist_id), title.to_string());
    }

    fn set_description(&self, playlist_id: &QString, text: &QString) {
        ctx().services.library.set_description(pid(playlist_id), text.to_string());
    }

    fn set_public(&self, playlist_id: &QString, value: bool) {
        ctx().services.library.set_public(pid(playlist_id), value);
    }

    fn follow_playlist(&self, playlist_id: &QString) {
        ctx().services.library.follow_playlist(pid(playlist_id));
    }

    fn unfollow_playlist(&self, playlist_id: &QString) {
        ctx().services.library.unfollow_playlist(pid(playlist_id));
    }

    // --- likes, saved ---

    fn is_liked(&self, track_id: &QString) -> bool {
        ctx().services.library.is_liked(&track_id.to_string())
    }

    fn toggle_like(&self, track: &QVariantMap) {
        if let Some(track) = track_from_map(track) {
            ctx().services.library.toggle_like(track);
        }
    }

    fn dislike(&self, track: &QVariantMap) {
        if let Some(track) = track_from_map(track) {
            ctx().services.library.dislike(track);
        }
    }

    fn is_saved(&self, uri: &QString) -> bool {
        ctx().services.library.is_saved(&uri.to_string())
    }

    fn toggle_saved(&self, uri: &QString) {
        ctx().services.library.toggle_saved(uri.to_string());
    }

    // --- "Library" screen ---

    fn items(mut self: Pin<&mut Self>, kind: &QString, owner: &QString, sort: &QString) -> *mut cxx_qt::QObject {
        let (kind, owner, sort) = (kind.to_string(), owner.to_string(), sort.to_string());
        let key = format!("{kind}|{owner}|{sort}");
        let rows = to_rows(&ctx().services.library.entries(LibraryKind::from_name(&kind), &owner, &sort));
        let this = self.as_mut().rust_mut().get_mut();
        let model = this.entry_models.entry(key).or_insert_with(|| DictListModel::create(ModelKind::Library));
        with_model(model, |m| m.sync_rows(rows, ENTRY_KEY));
        model_ptr(model)
    }

    fn recent_entries(mut self: Pin<&mut Self>, limit: i32) -> *mut cxx_qt::QObject {
        let limit = usize::try_from(limit.max(1)).unwrap_or(20).min(200);
        let key = format!("recent|{limit}");
        let library = &ctx().services.library;
        let rows = to_rows(&library.recent_entries(limit));
        let loading = library.playlists_loading();
        let this = self.as_mut().rust_mut().get_mut();
        let model = this.entry_models.entry(key).or_insert_with(|| DictListModel::create(ModelKind::Library));
        with_model(model, |mut m| {
            m.as_mut().sync_rows(rows, ENTRY_KEY);
            m.set_loading(loading);
        });
        model_ptr(model)
    }

    fn is_pinned(&self, uri: &QString) -> bool {
        ctx().services.library.is_pinned(&uri.to_string())
    }

    fn toggle_pin(&self, uri: &QString) {
        ctx().services.library.toggle_pin(uri.to_string());
    }

    // --- synchronization with the service ---

    fn sync_error(mut self: Pin<&mut Self>) {
        let text = QString::from(ctx().services.library.error_text().as_str());
        if self.error_text != text {
            self.as_mut().rust_mut().error_text = text;
            self.error_changed();
        }
    }

    fn sync_revisions(mut self: Pin<&mut Self>) {
        let library = &ctx().services.library;
        let likes = library.likes_revision() as i32;
        if self.likes_revision != likes {
            self.as_mut().rust_mut().likes_revision = likes;
            self.as_mut().likes_changed();
        }
        let saved = library.saved_revision() as i32;
        if self.saved_revision != saved {
            self.as_mut().rust_mut().saved_revision = saved;
            self.as_mut().saved_changed();
        }
        let pins = library.pins_revision() as i32;
        if self.pins_revision != pins {
            self.as_mut().rust_mut().pins_revision = pins;
            self.as_mut().pins_changed();
        }
        let (loaded, total) = library.loading_pages();
        if (self.pages_loaded, self.pages_total) != (loaded as i32, total as i32) {
            let mut this = self.as_mut().rust_mut();
            this.pages_loaded = loaded as i32;
            this.pages_total = total as i32;
            self.as_mut().pages_changed();
        }
    }

    /// Re-fill the items()/folderChildren() models according to their keys.
    fn sync_entries(mut self: Pin<&mut Self>) {
        let library = &ctx().services.library;
        let this = self.as_mut().rust_mut().get_mut();
        for (key, model) in this.entry_models.iter_mut() {
            // "recent|<limit>" — a summary across all sections for the side panel
            if let Some(limit) = key.strip_prefix("recent|") {
                let limit = limit.parse::<usize>().unwrap_or(20);
                let rows = to_rows(&library.recent_entries(limit));
                let loading = library.playlists_loading();
                with_model(model, |mut m| {
                    m.as_mut().sync_rows(rows, ENTRY_KEY);
                    m.set_loading(loading);
                });
                continue;
            }
            let mut parts = key.splitn(3, '|');
            let (kind, owner, sort) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
            let rows = to_rows(&library.entries(LibraryKind::from_name(kind), owner, sort));
            with_model(model, |m| m.sync_rows(rows, ENTRY_KEY));
        }
        self.entries_changed();
    }

    /// All content again (startup and missed events).
    fn resync(mut self: Pin<&mut Self>) {
        let library = &ctx().services.library;
        {
            let this = self.as_mut().rust_mut().get_mut();
            with_model(&mut this.playlists, |mut m| {
                m.as_mut().reset_rows(to_rows(&library.playlists()));
                m.set_loading(library.playlists_loading());
            });
            with_model(&mut this.liked, |mut m| {
                m.as_mut().reset_rows(to_rows(&library.liked()));
                m.set_loading(library.liked_loading());
            });
            with_model(&mut this.albums, |m| m.reset_rows(to_rows(&library.albums())));
            with_model(&mut this.artists, |m| m.reset_rows(to_rows(&library.artists())));
            with_model(&mut this.shows, |m| m.reset_rows(to_rows(&library.shows())));
        }
        let ids: Vec<PlaylistId> = self.playlist_models.keys().cloned().collect();
        // Re-filling the cache for all models, not just empty ones: missed events (Lagged) could have left
        // an extra or missing row in a non-empty model, and load_playlist for a complete page
        // will return Idle and it will not be re-read. The first re-fill event is Reset: the model is replaced,
        // not appended to, so repeating it over the current rows is safe.
        for id in &ids {
            library.replay_playlist(id);
        }
        for id in ids {
            library.load_playlist(id);
        }
        self.as_mut().sync_entries();
        self.as_mut().sync_error();
        self.sync_revisions();
    }

    /// Recent tracks — from history (duplicates by uri removed, the last 50).
    fn sync_recent(mut self: Pin<&mut Self>) {
        let mut seen = std::collections::HashSet::new();
        let tracks: Vec<_> = ctx()
            .services
            .history
            .recent(200)
            .into_iter()
            .map(|record| record.track)
            .filter(|track| seen.insert(track.play_uri()))
            .take(50)
            .collect();
        let this = self.as_mut().rust_mut().get_mut();
        with_model(&mut this.recent, |m| m.reset_rows(to_rows(&tracks)));
    }

    fn on_auth(mut self: Pin<&mut Self>, _delivery: Delivery<AuthEvent>) {
        let context = liked_context();
        with_model(&mut self.as_mut().rust_mut().get_mut().liked, |m| m.set_context(context));
    }

    fn on_history(self: Pin<&mut Self>, _delivery: Delivery<HistoryEvent>) {
        self.sync_recent();
    }

    fn on_event(mut self: Pin<&mut Self>, delivery: LibraryDelivery) {
        let event = match delivery {
            LibraryDelivery::Lagged => return self.resync(),
            LibraryDelivery::Ready(event) => event,
            // the rows are already serialized outside the GUI — just apply them
            LibraryDelivery::Tracks { id, change, permit: _permit } => {
                let this = self.as_mut().rust_mut().get_mut();
                if let Some(model) = this.playlist_models.get_mut(&id) {
                    with_model(model, |m| apply_row_change(m, &change));
                }
                return;
            }
        };
        let this = self.as_mut().rust_mut().get_mut();
        match event {
            LibraryEvent::Playlists(change) => with_model(&mut this.playlists, |m| apply_list_change(m, &change)),
            LibraryEvent::Liked(change) => with_model(&mut this.liked, |m| apply_list_change(m, &change)),
            LibraryEvent::Albums(change) => with_model(&mut this.albums, |m| apply_list_change(m, &change)),
            LibraryEvent::Artists(change) => with_model(&mut this.artists, |m| apply_list_change(m, &change)),
            LibraryEvent::Shows(change) => with_model(&mut this.shows, |m| apply_list_change(m, &change)),
            LibraryEvent::PlaylistTracks { id, change } => {
                if let Some(model) = this.playlist_models.get_mut(&id) {
                    with_model(model, |m| apply_list_change(m, &change));
                }
            }
            LibraryEvent::PlaylistInfo(id) => {
                if let Some(model) = this.playlist_models.get_mut(&id) {
                    with_model(model, |m| m.set_context(playlist_context(&id)));
                }
                self.playlist_info_changed(QString::from(id.as_str()))
            }
            LibraryEvent::EntriesChanged => {
                // loading more section pages ("3 of 5") also changes the page counter
                self.as_mut().sync_entries();
                self.sync_revisions()
            }
            LibraryEvent::LikesChanged | LibraryEvent::SavedChanged | LibraryEvent::PinsChanged => {
                self.sync_revisions()
            }
            LibraryEvent::ErrorChanged => self.sync_error(),
            LibraryEvent::Notice(text) => self.notice(QString::from(text.as_str())),
            LibraryEvent::Feedback { .. } => {}
        }
    }
}

impl cxx_qt::Initialize for qobject::Library {
    fn initialize(mut self: Pin<&mut Self>) {
        let events = ctx().services.library.subscribe();
        let history = ctx().services.history.subscribe();
        let auth = ctx().services.auth.subscribe();
        self.as_mut().resync();
        self.as_mut().sync_recent();
        self.as_mut().on_auth(Delivery::Lagged);
        forward_library(self.qt_thread(), events);
        forward_events(self.qt_thread(), history, Self::on_history);
        forward_events(self.qt_thread(), auth, Self::on_auth);
    }
}
