//! Catalogue: search, album, artist via ytmusicapi.

use std::sync::Arc;

use md3_model::{AlbumItem, ArtistItem, EpisodeItem, PlaylistItem, SearchBest, SearchTotals, ShowItem, TrackDetails, TrackItem};
use serde_json::Value;

use yt_player::PlayerHandle;

use crate::auth::AuthService;
use crate::library::LibraryService;
use crate::ServiceError;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchKind {
    #[default]
    All,
    Tracks,
    Albums,
    Artists,
    Playlists,
    Shows,
    Episodes,
}
impl SearchKind {
    pub fn from_name(name: &str) -> Self {
        match name {
            "tracks" => Self::Tracks,
            "albums" => Self::Albums,
            "artists" => Self::Artists,
            "playlists" => Self::Playlists,
            "shows" => Self::Shows,
            "episodes" => Self::Episodes,
            _ => Self::All,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Tracks => "tracks",
            Self::Albums => "albums",
            Self::Artists => "artists",
            Self::Playlists => "playlists",
            Self::Shows => "shows",
            Self::Episodes => "episodes",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AlbumPage { pub info: md3_model::AlbumInfo, pub tracks: Vec<TrackItem> }
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArtistPage { pub info: md3_model::ArtistInfo, pub popular: Vec<TrackItem>, pub albums: Vec<AlbumItem>, pub also_albums: Vec<AlbumItem>, pub similar: Vec<ArtistItem> }
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchResults {
    pub tracks: Vec<TrackItem>, pub albums: Vec<AlbumItem>, pub artists: Vec<ArtistItem>,
    pub playlists: Vec<PlaylistItem>, pub shows: Vec<ShowItem>, pub episodes: Vec<EpisodeItem>,
    pub totals: SearchTotals, pub best: Option<SearchBest>,
}
impl SearchResults { pub fn is_empty(&self) -> bool { self.tracks.is_empty() && self.albums.is_empty() && self.artists.is_empty() && self.playlists.is_empty() && self.shows.is_empty() && self.episodes.is_empty() } }
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShowPage { pub info: ShowItem, pub episodes: Vec<EpisodeItem>, pub total: i64 }
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EpisodePage { pub info: EpisodeItem, pub show: ShowItem, pub more: Vec<EpisodeItem> }

pub struct CatalogService {
    auth: Arc<AuthService>,
    library: Arc<LibraryService>,
    player: PlayerHandle,
}

impl CatalogService {
    pub fn new(auth: Arc<AuthService>, library: Arc<LibraryService>, player: PlayerHandle) -> Arc<Self> {
        Arc::new(Self { auth, library, player })
    }

    pub async fn search(&self, query: &str, _kind: SearchKind) -> Result<SearchResults, ServiceError> {
        let q = query.trim();
        if q.is_empty() { return Ok(SearchResults::default()); }
        let api = self.auth.api();
        let filter = match _kind {
            SearchKind::Tracks => Some("songs"),
            SearchKind::Albums => Some("albums"),
            SearchKind::Artists => Some("artists"),
            SearchKind::Playlists => Some("playlists"),
            _ => None,
        };
        let v = api.search(q, filter, 20).await.map_err(ServiceError::from)?;
        let list: Vec<serde_json::Value> = v.as_array().cloned().unwrap_or_default();
        let mut out = SearchResults::default();
        for item in &list {
            let rt = item.get("resultType").and_then(|t| t.as_str()).unwrap_or("");
            match rt {
                "song" | "video" => {
                    if item.get("videoType").and_then(|t| t.as_str()).map_or(false, |vt| vt.contains("PODCAST")) {
                        out.episodes.push(yt_api::map::episode_item(item));
                    } else {
                        out.tracks.push(yt_api::map::track_item(item));
                    }
                }
                "album" => out.albums.push(yt_api::map::album_item(item)),
                "artist" | "channel" => out.artists.push(yt_api::map::artist_item(item)),
                "playlist" => {
                    if item.get("browseId").and_then(|b| b.as_str()).map_or(false, |b| b.starts_with("MPSP")) {
                        out.shows.push(yt_api::map::show_item(item));
                    } else {
                        out.playlists.push(yt_api::map::playlist_item(item));
                    }
                }
                "show" => out.shows.push(yt_api::map::show_item(item)),
                "episode" => out.episodes.push(yt_api::map::episode_item(item)),
                _ => {
                    if item.get("videoId").is_some() {
                        if item.get("videoType").and_then(|t| t.as_str()).map_or(false, |vt| vt.contains("PODCAST")) {
                            out.episodes.push(yt_api::map::episode_item(item));
                        } else {
                            out.tracks.push(yt_api::map::track_item(item));
                        }
                    } else if item.get("browseId").is_some() && item.get("title").is_some() {
                        let bid = item.get("browseId").and_then(|b| b.as_str()).unwrap_or("");
                        if bid.starts_with("MPSP") {
                            out.shows.push(yt_api::map::show_item(item));
                        } else if bid.starts_with("MPREb_") {
                            out.albums.push(yt_api::map::album_item(item));
                        } else {
                            out.playlists.push(yt_api::map::playlist_item(item));
                        }
                    }
                }
            }
        }
        out.totals = SearchTotals {
            tracks: out.tracks.len() as i64,
            albums: out.albums.len() as i64,
            artists: out.artists.len() as i64,
            playlists: out.playlists.len() as i64,
            shows: out.shows.len() as i64,
            episodes: out.episodes.len() as i64,
        };
        if let Some(first) = out.tracks.first() {
            if let Ok(map) = serde_json::to_value(first) {
                if let Some(obj) = map.as_object().cloned() {
                    out.best = Some(SearchBest { kind: "track".into(), item: obj });
                }
            }
        }
        // Remember these items so a later bare-URI queue entry shows the real
        // name instantly (in-session only, always fresh API data).
        crate::data::known::remember(out.tracks.iter().cloned());
        Ok(out)
    }

    pub async fn album(&self, album_id: &str) -> Result<AlbumPage, ServiceError> {
        let api = self.auth.api();
        let v = api.get_album(album_id).await.map_err(ServiceError::from)?;
        let (info, tracks) = yt_api::map::album_page(&v);
        crate::data::known::remember(tracks.iter().cloned());
        Ok(AlbumPage { info, tracks })
    }

    pub async fn artist(&self, artist_id: &str) -> Result<ArtistPage, ServiceError> {
        let api = self.auth.api();
        let v = api.get_artist(artist_id).await.map_err(ServiceError::from)?;
        let info = yt_api::map::artist_info(&v);
        // ytmusicapi get_artist: songs, albums, singles, related
        let popular: Vec<TrackItem> = v.get("songs").and_then(|s| s.as_array())
            .map(|a| a.iter().map(yt_api::map::track_item).collect()).unwrap_or_default();
        let mut albums: Vec<AlbumItem> = Vec::new();
        for k in ["albums", "singles"] {
            if let Some(list) = v.get(k).and_then(|s| s.as_array()) {
                for item in list { albums.push(yt_api::map::album_item(item)); }
            }
        }
        let similar: Vec<ArtistItem> = v.get("related").and_then(|s| s.as_array())
            .map(|a| a.iter().map(yt_api::map::artist_item).collect()).unwrap_or_default();
        crate::data::known::remember(popular.iter().cloned());
        Ok(ArtistPage { info, popular, albums, also_albums: vec![], similar })
    }

    pub async fn track(&self, track_id: &str) -> Result<TrackItem, ServiceError> {
        let api = self.auth.api();
        let v = api.get_song(track_id).await.map_err(ServiceError::from)?;
        let track = yt_api::map::track_item(&v);
        crate::data::known::remember(std::iter::once(track.clone()));
        Ok(track)
    }

    pub async fn track_details(&self, track_id: &str, _album_id: &str) -> Result<TrackDetails, ServiceError> {
        let track = self.track(track_id).await?;
        Ok(TrackDetails { track, popularity: -1, ..Default::default() })
    }

    pub async fn show(&self, show_id: &str) -> Result<ShowPage, ServiceError> {
        let api = self.auth.api();
        let v = api.get_podcast(show_id, 100).await.map_err(ServiceError::from)?;
        let info = yt_api::map::show_item(&v);
        let episodes: Vec<EpisodeItem> = v
            .get("episodes")
            .and_then(|e| e.as_array())
            .map(|a| a.iter().map(yt_api::map::episode_item).collect::<Vec<EpisodeItem>>())
            .unwrap_or_default();
        let total = episodes.len() as i64;
        // показ сохранён, если флаг пришёл с данными страницы
        self.library.note_saved(&info.uri, info.saved);
        Ok(ShowPage { info, episodes, total })
    }

    pub async fn show_episodes(&self, show_id: &str, _offset: u32) -> Result<Vec<EpisodeItem>, ServiceError> {
        let api = self.auth.api();
        let v = api.get_podcast(show_id, 100).await.map_err(ServiceError::from)?;
        let episodes: Vec<EpisodeItem> = v
            .get("episodes")
            .and_then(|e| e.as_array())
            .map(|a| a.iter().map(yt_api::map::episode_item).collect::<Vec<EpisodeItem>>())
            .unwrap_or_default();
        Ok(episodes)
    }

    pub async fn episode(&self, episode_id: &str) -> Result<EpisodePage, ServiceError> {
        let api = self.auth.api();
        let v = api.get_episode(episode_id).await.map_err(ServiceError::from)?;
        let info = yt_api::map::episode_item(&v);
        // playlistId ответа — идентификатор шоу (с префиксом MPSP)
        let show_id = v
            .get("playlistId")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim_start_matches("MPSP")
            .to_owned();
        let show = if !show_id.is_empty() {
            match api.get_podcast(&show_id, 1).await {
                Ok(sv) => {
                    let mut sh = yt_api::map::show_item(&sv);
                    sh.saved = v.get("saved").and_then(Value::as_bool).unwrap_or(false);
                    self.library.note_saved(&sh.uri, sh.saved);
                    sh
                }
                Err(_) => ShowItem {
                    show_id: show_id.clone(),
                    uri: format!("ytmusic:show:{show_id}"),
                    ..Default::default()
                },
            }
        } else {
            ShowItem::default()
        };
        let more: Vec<EpisodeItem> = v
            .get("episodes")
            .and_then(|e| e.as_array())
            .map(|a| a.iter().map(yt_api::map::episode_item).collect::<Vec<EpisodeItem>>())
            .unwrap_or_default()
            .into_iter()
            .filter(|e| e.episode_id != info.episode_id)
            .collect();
        Ok(EpisodePage { info, show, more })
    }
    pub fn forget(&self) {}
    pub fn player(&self) -> &PlayerHandle { &self.player }
}
