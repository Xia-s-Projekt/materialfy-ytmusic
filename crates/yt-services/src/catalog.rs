//! Catalogue: search, album, artist via ytmusicapi.

use std::sync::Arc;

use md3_model::{AlbumItem, ArtistItem, EpisodeItem, PlaylistItem, SearchBest, SearchTotals, ShowItem, TrackDetails, TrackItem};
use tokio::runtime::Handle;

use yt_player::PlayerHandle;

use crate::auth::AuthService;
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
    player: PlayerHandle,
}

impl CatalogService {
    pub fn new(auth: Arc<AuthService>, player: PlayerHandle) -> Arc<Self> {
        Arc::new(Self { auth, player })
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
                "song" | "video" => out.tracks.push(yt_api::map::track_item(item)),
                "album" => out.albums.push(yt_api::map::album_item(item)),
                "artist" | "channel" => out.artists.push(yt_api::map::artist_item(item)),
                "playlist" => out.playlists.push(yt_api::map::playlist_item(item)),
                _ => {
                    if item.get("videoId").is_some() { out.tracks.push(yt_api::map::track_item(item)); }
                    else if item.get("browseId").is_some() && item.get("title").is_some() { out.albums.push(yt_api::map::album_item(item)); }
                }
            }
        }
        out.totals = SearchTotals {
            tracks: out.tracks.len() as i64,
            albums: out.albums.len() as i64,
            artists: out.artists.len() as i64,
            playlists: out.playlists.len() as i64,
            shows: 0, episodes: 0,
        };
        if let Some(first) = out.tracks.first() {
            if let Ok(map) = serde_json::to_value(first) {
                if let Some(obj) = map.as_object().cloned() {
                    out.best = Some(SearchBest { kind: "track".into(), item: obj });
                }
            }
        }
        Ok(out)
    }

    pub async fn album(&self, album_id: &str) -> Result<AlbumPage, ServiceError> {
        let api = self.auth.api();
        let v = api.get_album(album_id).await.map_err(ServiceError::from)?;
        let (info, tracks) = yt_api::map::album_page(&v);
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
        Ok(ArtistPage { info, popular, albums, also_albums: vec![], similar })
    }

    pub async fn track(&self, track_id: &str) -> Result<TrackItem, ServiceError> {
        let api = self.auth.api();
        let v = api.get_song(track_id).await.map_err(ServiceError::from)?;
        Ok(yt_api::map::track_item(&v))
    }

    pub async fn track_details(&self, track_id: &str, _album_id: &str) -> Result<TrackDetails, ServiceError> {
        let track = self.track(track_id).await?;
        Ok(TrackDetails { track, popularity: -1, ..Default::default() })
    }

    pub async fn show(&self, _show_id: &str) -> Result<ShowPage, ServiceError> {
        Err(ServiceError::Other("Podcasts/shows are not available via ytmusicapi in this build".into()))
    }
    pub async fn show_episodes(&self, _show_id: &str, _offset: u32) -> Result<Vec<EpisodeItem>, ServiceError> {
        Err(ServiceError::Other("Podcasts/shows are not available via ytmusicapi in this build".into()))
    }
    pub async fn episode(&self, _episode_id: &str) -> Result<EpisodePage, ServiceError> {
        Err(ServiceError::Other("Podcasts/shows are not available via ytmusicapi in this build".into()))
    }
    pub fn forget(&self) {}
    pub fn player(&self) -> &PlayerHandle { &self.player }
}
