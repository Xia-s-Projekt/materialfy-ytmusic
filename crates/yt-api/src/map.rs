//! ytmusicapi JSON → md3-model items.
//! ytmusicapi shapes (see ytmusicapi docs): search results, library songs,
//! liked songs, albums, artists, playlists, home shelves.

use serde_json::Value;

use md3_model as m;

use crate::util::{duration_ms, pick_thumbnail, strip_html, video_id};

pub const COVER_PX: u32 = 300;
pub const COVER_LARGE_PX: u32 = 640;

fn s(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

fn arr(v: &Value, key: &str) -> Vec<Value> {
    v.get(key)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn thumbs(v: &Value) -> Vec<Value> {
    arr(v, "thumbnails")
}

fn artists(v: &Value) -> Vec<m::ArtistRef> {
    arr(v, "artists")
        .iter()
        .map(|a| m::ArtistRef {
            id: a.get("name").and_then(Value::as_str).map(|_| s(a, "browseId").if_empty(s(a, "id"))).unwrap_or_default(),
            name: s(a, "name"),
        })
        .filter(|a| !a.name.is_empty())
        .collect()
}

trait IfEmpty {
    fn if_empty(self, fallback: String) -> String;
}
impl IfEmpty for String {
    fn if_empty(self, fallback: String) -> String {
        if self.is_empty() { fallback } else { self }
    }
}

fn artist_names(v: &Value) -> String {
    let names: Vec<String> = arr(v, "artists").iter().map(|a| s(a, "name")).collect();
    names.into_iter().filter(|n| !n.is_empty()).collect::<Vec<_>>().join(", ")
}

fn duration_of(v: &Value) -> i64 {
    if let Some(d) = v.get("duration_seconds").and_then(Value::as_i64) {
        return d * 1000;
    }
    if let Some(d) = v.get("duration").and_then(Value::as_str) {
        return duration_ms(d);
    }
    if let Some(d) = v.get("duration_ms").and_then(Value::as_i64) {
        return d;
    }
    if let Some(d) = v.get("length").and_then(Value::as_str) {
        return duration_ms(d);
    }
    0
}

fn explicit_of(v: &Value) -> bool {
    v.get("isExplicit").and_then(Value::as_bool).unwrap_or(false)
        || v.get("explicit").and_then(Value::as_bool).unwrap_or(false)
}

/// Song/track item (search `songs`/`videos`, library, liked, watch playlist).
pub fn track_item(v: &Value) -> m::TrackItem {
    let id = video_id(v);
    let thumb = thumbs(v);
    m::TrackItem {
        track_id: id.clone(),
        album_id: s(v, "album").if_empty(album_browse_id(v)),
        title: s(v, "title"),
        version: String::new(),
        artists: artist_names(v),
        artist_refs: artists(v),
        album: s(v, "album"),
        cover: pick_thumbnail(&thumb, COVER_PX),
        cover_large: pick_thumbnail(&thumb, COVER_LARGE_PX),
        duration_ms: duration_of(v),
        explicit: explicit_of(v),
        available: true,
        uri: if id.is_empty() { String::new() } else { format!("ytmusic:track:{id}") },
        is_episode: false,
        show_id: String::new(),
        added_at: s(v, "addedAt").if_empty(s(v, "date")),
        added_by: String::new(),
        play_count: s(v, "views").parse::<i64>().unwrap_or(0),
    }
}

fn album_browse_id(v: &Value) -> String {
    arr(v, "album")
        .first()
        .and_then(|a| a.get("browseId").and_then(Value::as_str))
        .unwrap_or("")
        .to_string()
}

/// Album (search / library / artist releases).
pub fn album_item(v: &Value) -> m::AlbumItem {
    let id = s(v, "browseId").if_empty(s(v, "playlistId"));
    let thumb = thumbs(v);
    let year = s(v, "year").parse::<i64>().unwrap_or(0);
    let kind = s(v, "type");
    m::AlbumItem {
        album_id: id.clone(),
        uri: if id.is_empty() { String::new() } else { format!("ytmusic:album:{id}") },
        title: s(v, "title"),
        version: String::new(),
        artists: artist_names(v),
        artist_refs: artists(v),
        year,
        kind: if kind.is_empty() { "Album".to_string() } else { kind },
        track_count: v.get("trackCount").and_then(Value::as_i64).unwrap_or(0),
        cover: pick_thumbnail(&thumb, COVER_PX),
        cover_large: pick_thumbnail(&thumb, COVER_LARGE_PX),
        explicit: explicit_of(v),
        saved: false,
    }
}

pub fn artist_item(v: &Value) -> m::ArtistItem {
    let id = s(v, "browseId").if_empty(s(v, "channelId"));
    let thumb = thumbs(v);
    m::ArtistItem {
        artist_id: id.clone(),
        uri: if id.is_empty() { String::new() } else { format!("ytmusic:artist:{id}") },
        name: s(v, "title").if_empty(s(v, "name")),
        cover: pick_thumbnail(&thumb, COVER_PX),
        cover_large: pick_thumbnail(&thumb, COVER_LARGE_PX),
        genres: s(v, "genres"),
        followers: v.get("subscribers").and_then(Value::as_i64).unwrap_or(0),
    }
}

pub fn playlist_item(v: &Value) -> m::PlaylistItem {
    let id = s(v, "playlistId").if_empty(s(v, "browseId"));
    let thumb = thumbs(v);
    m::PlaylistItem {
        playlist_id: id.clone(),
        uri: if id.is_empty() { String::new() } else { format!("ytmusic:playlist:{id}") },
        title: s(v, "title"),
        description: s(v, "description"),
        track_count: v.get("songCount").and_then(Value::as_i64).unwrap_or(
            v.get("trackCount").and_then(Value::as_i64).unwrap_or(0),
        ),
        cover: pick_thumbnail(&thumb, COVER_PX),
        owner: s(v, "author"),
        owner_id: s(v, "channelId"),
        can_edit: false,
        collaborative: false,
        public: true,
        snapshot_id: String::new(),
        pinned: false,
    }
}

/// get_album → AlbumInfo + tracks (ytmusicapi `get_album` returns dict with `tracks`).
pub fn album_page(album: &Value) -> (m::AlbumInfo, Vec<m::TrackItem>) {
    let mut info = m::AlbumInfo {
        item: album_item(album),
        label: s(album, "label"),
        genre: s(album, "genre"),
        duration_ms: 0,
        release_date: s(album, "releaseDate").if_empty(s(album, "date")),
        copyright: s(album, "copyright"),
        copyrights: vec![],
        courtesy_line: String::new(),
    };
    let tracks: Vec<m::TrackItem> = arr(album, "tracks").iter().map(track_item).collect();
    info.duration_ms = tracks.iter().map(|t| t.duration_ms).sum();
    (info, tracks)
}

/// get_artist → ArtistInfo.
pub fn artist_info(v: &Value) -> m::ArtistInfo {
    m::ArtistInfo {
        item: artist_item(v),
        listeners: 0,
        followed: false,
        biography: strip_html(&s(v, "description")),
        header_image: pick_thumbnail(&thumbs(v), COVER_LARGE_PX),
    }
}

/// get_home shelves → md3-model HomeShelf items (raw JSON values for QML).
pub fn home_shelves(home: &[Value]) -> Vec<m::HomeShelf> {
    home.iter()
        .map(|row| {
            let contents = row.get("contents").cloned().unwrap_or(Value::Array(vec![]));
            let items = match contents {
                Value::Array(list) => list.into_iter().map(|v| serde_json::to_value(&v).unwrap_or(Value::Null)).collect(),
                _ => vec![],
            };
            m::HomeShelf {
                key: s(row, "title"),
                title: s(row, "title"),
                kind: "tracks".to_string(),
                items,
            }
        })
        .collect()
}

/// uri kind/id helpers (ytmusic:track:ID).
pub fn uri_kind(uri: &str) -> Option<&'static str> {
    match uri.split(':').next() {
        Some("ytmusic") => {
            let kind = uri.split(':').nth(1)?;
            match kind {
                "track" => Some("track"),
                "album" => Some("album"),
                "artist" => Some("artist"),
                "playlist" => Some("playlist"),
                _ => None,
            }
        }
        _ => None,
    }
}

pub fn uri_id(uri: &str) -> Option<String> {
    uri.split(':').last().map(str::to_string).filter(|s| !s.is_empty())
}

/// Stem for open in browser (replaces Spotify util).
pub fn open_url(uri: &str) -> Option<String> {
    let kind = uri_kind(uri)?;
    let id = uri_id(uri)?;
    match kind {
        "track" => Some(format!("https://music.youtube.com/watch?v={id}")),
        "playlist" => Some(format!("https://music.youtube.com/playlist?list={id}")),
        "album" => Some(format!("https://music.youtube.com/watch?v=&list={id}")),
        "artist" => Some(format!("https://music.youtube.com/channel/{id}")),
        _ => None,
    }
}

/// Queue row from a track.
pub fn queue_row(track: m::TrackItem, section: m::QueueSection, uid: u64) -> m::QueueRow {
    m::QueueRow { section, uid, track }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn maps_song_like_search_result() {
        let v = serde_json::json!({
            "videoId": "abc", "title": "Song",
            "artists": [{"name": "A", "browseId": "UC1"}],
            "album": "Alb",
            "duration_seconds": 225,
            "thumbnails": [{"url":"sm","width":60},{"url":"lg","width":1080}]
        });
        let t = track_item(&v);
        assert_eq!(t.track_id, "abc");
        assert_eq!(t.title, "Song");
        assert_eq!(t.artists, "A");
        assert_eq!(t.duration_ms, 225_000);
        assert_eq!(t.cover_large, "lg");
        assert_eq!(t.uri, "ytmusic:track:abc");
    }
    #[test]
    fn uri_roundtrips() {
        assert_eq!(uri_kind("ytmusic:track:abc"), Some("track"));
        assert_eq!(uri_id("ytmusic:track:abc").as_deref(), Some("abc"));
        assert_eq!(open_url("ytmusic:track:abc").as_deref(), Some("https://music.youtube.com/watch?v=abc"));
    }
}
