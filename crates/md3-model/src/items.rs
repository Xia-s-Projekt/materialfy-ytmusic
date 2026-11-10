//! Flat items for QML. serde camelCase; field names = model roles / QML dictionary keys.
//! Covers: `cover` ~300 px (grids, rows), `cover_large` 640 px (headers, NowPlaying); empty — none.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct ArtistRef {
    pub id: String,
    pub name: String,
}

/// A track or a podcast episode. QML keys (TRACK_KEYS): trackId, albumId, title, version, artists, artistRefs, album,
/// cover, coverLarge, durationMs, explicit, available, uri, isEpisode, showId, addedAt, addedBy, playCount.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TrackItem {
    /// base62 id of the track (for an episode — the episode's id)
    pub track_id: String,
    pub album_id: String,
    pub title: String,
    /// Spotify has no separate version field — always ""
    pub version: String,
    /// names joined with ", "
    pub artists: String,
    pub artist_refs: Vec<ArtistRef>,
    /// the album title (for an episode — the show's title)
    pub album: String,
    pub cover: String,
    pub cover_large: String,
    pub duration_ms: i64,
    pub explicit: bool,
    /// is_playable != false; unknown → true
    pub available: bool,
    /// spotify:track:ID | spotify:episode:ID
    pub uri: String,
    pub is_episode: bool,
    /// for an episode — the show's id, otherwise ""
    pub show_id: String,
    /// playlist row: ISO time it was added ("" — unknown)
    pub added_at: String,
    /// collaborative playlist row: the name of whoever added it ("" — unknown/not needed)
    pub added_by: String,
    /// plays on Spotify (GraphQL gateway: album and artist pages); 0 — unknown
    pub play_count: i64,
    /// YouTube Music playlist row: the playlist entry id (needed for
    /// deletion and reordering; "" — not from a playlist or unavailable)
    pub set_video_id: String,
}

impl TrackItem {
    /// URI for the queue/playback: `uri` or spotify:track:<trackId>.
    pub fn play_uri(&self) -> String {
        if !self.uri.is_empty() {
            self.uri.clone()
        } else if self.is_episode {
            format!("spotify:episode:{}", self.track_id)
        } else {
            format!("spotify:track:{}", self.track_id)
        }
    }
}

/// Album kind label in Russian: Spotify's album_type (+ the track count for an EP).
pub fn album_kind_label(album_type: &str, total_tracks: i64) -> String {
    match album_type {
        "single" if total_tracks > 1 => "EP".to_owned(),
        "single" => mf_i18n::t("Single"),
        "compilation" => mf_i18n::t("Compilation"),
        "ep" => "EP".to_owned(),
        "audiobook" => mf_i18n::t("Audiobook"),
        "podcast" | "show" => mf_i18n::t("Podcast"),
        _ => mf_i18n::t("Album"),
    }
}

/// ALBUM_KEYS: albumId, uri, title, version, artists, artistRefs, year, kind, trackCount, cover, coverLarge, explicit, saved.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AlbumItem {
    pub album_id: String,
    pub uri: String,
    pub title: String,
    pub version: String,
    pub artists: String,
    pub artist_refs: Vec<ArtistRef>,
    /// the first 4 characters of release_date
    pub year: i64,
    /// album_kind_label
    pub kind: String,
    pub track_count: i64,
    pub cover: String,
    pub cover_large: String,
    pub explicit: bool,
    /// in the user's library (if known)
    pub saved: bool,
}

/// ARTIST_KEYS: artistId, uri, name, cover, coverLarge, genres, followers.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ArtistItem {
    pub artist_id: String,
    pub uri: String,
    pub name: String,
    pub cover: String,
    pub cover_large: String,
    /// genres joined with ", "
    pub genres: String,
    pub followers: i64,
}

/// PLAYLIST_KEYS: playlistId, uri, title, description, trackCount, cover, owner, ownerId, canEdit, collaborative,
/// public, snapshotId, pinned.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PlaylistItem {
    pub playlist_id: String,
    pub uri: String,
    pub title: String,
    pub description: String,
    pub track_count: i64,
    pub cover: String,
    /// the owner's name
    pub owner: String,
    pub owner_id: String,
    /// own, collaborative or by invitation (the rights from librespot's rootlist)
    pub can_edit: bool,
    pub collaborative: bool,
    pub public: bool,
    pub snapshot_id: String,
    /// pinned locally
    pub pinned: bool,
}

/// AlbumData.info: AlbumItem fields + label, genre, durationMs, releaseDate, copyright, copyrights, courtesyLine.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AlbumInfo {
    #[serde(flatten)]
    pub item: AlbumItem,
    pub label: String,
    /// often empty on Spotify
    pub genre: String,
    pub duration_ms: i64,
    /// "2001-03-12" | "2001-03" | "2001" — with whatever precision Spotify knows
    pub release_date: String,
    pub copyright: String,
    /// all copyright lines with a prefix: "© 2001 …", "℗ 2001 …"
    pub copyrights: Vec<String>,
    /// the "courtesy of …" line (GraphQL gateway), often empty
    pub courtesy_line: String,
}

/// Everything known about the track (the "About the track" window): TrackItem fields + number, disc, ISRC, popularity and details
/// about the album (release date, label, copyright holders).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TrackDetails {
    #[serde(flatten)]
    pub track: TrackItem,
    /// 0 — unknown
    pub track_number: i64,
    pub disc_number: i64,
    /// discs in the album (0 — unknown)
    pub disc_count: i64,
    /// tracks in the album (0 — unknown)
    pub album_track_count: i64,
    pub isrc: String,
    /// 0..100, -1 — unknown
    pub popularity: i64,
    /// the album's album_kind_label
    pub album_kind: String,
    pub release_date: String,
    pub label: String,
    pub copyrights: Vec<String>,
    pub courtesy_line: String,
}

/// ArtistData.info: ArtistItem fields + listeners, followed, biography, headerImage.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ArtistInfo {
    #[serde(flatten)]
    pub item: ArtistItem,
    /// monthly listeners (GraphQL gateway; the Web API does not return them — 0)
    pub listeners: i64,
    /// the user follows it
    pub followed: bool,
    /// biography without HTML (GraphQL gateway; otherwise empty)
    pub biography: String,
    /// the artist's wide header (GraphQL gateway; otherwise empty)
    pub header_image: String,
}

/// SHOW_KEYS: showId, uri, name, publisher, description, cover, coverLarge, totalEpisodes, saved, explicit.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ShowItem {
    pub show_id: String,
    pub uri: String,
    pub name: String,
    pub publisher: String,
    pub description: String,
    pub cover: String,
    pub cover_large: String,
    pub total_episodes: i64,
    pub saved: bool,
    pub explicit: bool,
}

/// EPISODE_KEYS: episodeId, uri, title, description, releaseDate, durationMs, explicit, resumeMs, fullyPlayed, saved,
/// cover, coverLarge, showId, showName, number.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EpisodeItem {
    pub episode_id: String,
    pub uri: String,
    pub title: String,
    pub description: String,
    /// ISO date "2026-10-03"
    pub release_date: String,
    pub duration_ms: i64,
    pub explicit: bool,
    /// where to resume from (resume_point), 0 — from the start
    pub resume_ms: i64,
    pub fully_played: bool,
    pub saved: bool,
    pub cover: String,
    pub cover_large: String,
    pub show_id: String,
    pub show_name: String,
    /// the episode number if known (otherwise 0)
    pub number: i64,
}

impl EpisodeItem {
    /// As a track row (the queue, the player).
    pub fn to_track(&self) -> TrackItem {
        TrackItem {
            track_id: self.episode_id.clone(),
            album_id: String::new(),
            title: self.title.clone(),
            version: String::new(),
            artists: self.show_name.clone(),
            artist_refs: Vec::new(),
            album: self.show_name.clone(),
            cover: self.cover.clone(),
            cover_large: self.cover_large.clone(),
            duration_ms: self.duration_ms,
            explicit: self.explicit,
            available: true,
            uri: self.uri.clone(),
            is_episode: true,
            show_id: self.show_id.clone(),
            ..Default::default()
        }
    }
}

/// DEVICE_KEYS: id, name, type, isActive, isThisComputer, isRestricted, supportsVolume, volume, source.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DeviceItem {
    pub id: String,
    pub name: String,
    /// computer | smartphone | tablet | tv | speaker | avr | stb | automobile | gameconsole | castvideo | castaudio | …
    /// (Spotify's Device.type in lower case)
    #[serde(rename = "type")]
    pub kind: String,
    pub is_active: bool,
    pub is_this_computer: bool,
    pub is_restricted: bool,
    pub supports_volume: bool,
    /// 0..100, -1 — unknown
    pub volume: i32,
    /// "api" — /me/player/devices, "zeroconf" — a receiver from the local network, not yet in the account
    pub source: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QueueSection {
    /// playing right now
    #[default]
    Current,
    /// "Next in queue" — placed by the user (Playing next)
    Manual,
    /// "Next from the context" (Next up)
    Context,
}

/// A Player.queue row (QUEUE_KEYS): section, uid + all track keys.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct QueueRow {
    pub section: QueueSection,
    /// unique per entry (duplicates allowed); stable while the row is in the queue
    pub uid: u64,
    #[serde(flatten)]
    pub track: TrackItem,
}

/// A library entry (LIBRARY_KEYS): kind, id, uri, title, subtitle, cover, covers, owner, ownerId, canEdit,
/// readOnly, pinned, trackCount, addedAt, playedAt, folderId.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LibraryEntry {
    /// liked | playlist | dj | folder | album | artist | show (| radio — only in the home shelves)
    pub kind: String,
    pub id: String,
    pub uri: String,
    pub title: String,
    /// "Playlist · You · 34 tracks" — assembled by the service
    pub subtitle: String,
    pub cover: String,
    /// up to 4 covers (the folder mosaic)
    pub covers: Vec<String>,
    pub owner: String,
    pub owner_id: String,
    pub can_edit: bool,
    /// a folder or someone else's playlist
    pub read_only: bool,
    pub pinned: bool,
    pub track_count: i64,
    /// ISO time it was added to the library, "" — unknown
    pub added_at: String,
    /// ISO time of the last play (local history), ""
    pub played_at: String,
    /// id of the parent rootlist folder, "" — the root
    pub folder_id: String,
}

/// An account for Auth.account: id, uri, displayName, email, country, product, premium, avatar.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Account {
    pub id: String,
    pub uri: String,
    pub display_name: String,
    pub email: String,
    pub country: String,
    /// "premium" | "free" | "open" | …
    pub product: String,
    /// product == "premium"
    pub premium: bool,
    pub avatar: String,
}

/// A radio seed in the home orbit: uri (spotify:track|artist|album|playlist:ID), label, cover, kind.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RadioSeed {
    pub uri: String,
    pub label: String,
    pub cover: String,
    /// track | artist | album | playlist
    pub kind: String,
}

/// A group of seeds: key (recent | artists | collections | any), title, items.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RadioSeedGroup {
    pub key: String,
    pub title: String,
    pub items: Vec<RadioSeed>,
}

/// A home shelf: key (quick | recent | madeForYou | podcasts | topArtists | topTracks), title,
/// kind (tiles | tracks | episodes | artists), items — rows of the matching *Item / LibraryEntry.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HomeShelf {
    pub key: String,
    pub title: String,
    pub kind: String,
    pub items: Vec<serde_json::Value>,
}

/// Search totals: tracks, albums, artists, playlists, shows, episodes.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SearchTotals {
    pub tracks: i64,
    pub albums: i64,
    pub artists: i64,
    pub playlists: i64,
    pub shows: i64,
    pub episodes: i64,
}

/// The best result: {type: track|album|artist|playlist|show, item: {…}} (a heuristic, Spotify has none).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SearchBest {
    #[serde(rename = "type")]
    pub kind: String,
    pub item: serde_json::Map<String, serde_json::Value>,
}
