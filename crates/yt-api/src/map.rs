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
        set_video_id: s(v, "setVideoId"),
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
        // likeStatus альбома: LIKE — в библиотеке
        saved: s(v, "likeStatus") == "LIKE",
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

/// Podcast (search, library subscriptions, podcast page header).
pub fn show_item(v: &Value) -> m::ShowItem {
    // browseId подкаста — «MPSP» + id плейлиста; get_podcast ждёт голый id.
    let id = s(v, "browseId")
        .if_empty(s(v, "podcastId"))
        .trim_start_matches("MPSP")
        .to_owned();
    let thumb = thumbs(v);
    let author = v.get("author").or_else(|| v.get("channel"));
    m::ShowItem {
        show_id: id.clone(),
        uri: if id.is_empty() { String::new() } else { format!("ytmusic:show:{id}") },
        name: s(v, "title").if_empty(s(v, "name")),
        publisher: s(v, "publisher").if_empty(s(author.unwrap_or(&Value::Null), "name")),
        description: s(v, "description"),
        cover: pick_thumbnail(&thumb, COVER_PX),
        cover_large: pick_thumbnail(&thumb, COVER_LARGE_PX),
        total_episodes: v.get("episodeCount").and_then(Value::as_i64).unwrap_or(0),
        // get_podcast отвечает saved; в поиске и библиотеке флага нет
        saved: v.get("saved").and_then(Value::as_bool).unwrap_or(false),
        explicit: explicit_of(v),
    }
}

/// Выпуск подкаста (список подкаста, поиск, страница выпуска).
pub fn episode_item(v: &Value) -> m::EpisodeItem {
    let id = s(v, "videoId");
    let thumb = thumbs(v);
    let show = v.get("podcast").or_else(|| v.get("show"));
    let show_id = s(v, "showId")
        .if_empty(s(show.unwrap_or(&Value::Null), "id"))
        .trim_start_matches("MPSP")
        .to_owned();
    m::EpisodeItem {
        episode_id: id.clone(),
        uri: if id.is_empty() { String::new() } else { format!("ytmusic:episode:{id}") },
        title: s(v, "title"),
        description: s(v, "description"),
        release_date: s(v, "date").if_empty(s(v, "releaseDate")),
        duration_ms: duration_of(v),
        explicit: explicit_of(v),
        resume_ms: 0,
        fully_played: false,
        saved: false,
        cover: pick_thumbnail(&thumb, COVER_PX),
        cover_large: pick_thumbnail(&thumb, COVER_LARGE_PX),
        show_id,
        show_name: s(v, "showName").if_empty(s(show.unwrap_or(&Value::Null), "name")),
        number: v.get("index").and_then(Value::as_i64).unwrap_or(0),
    }
}

pub fn playlist_item(v: &Value) -> m::PlaylistItem {
    let id = s(v, "playlistId").if_empty(s(v, "browseId"));
    let thumb = thumbs(v);
    let author = v.get("author");
    m::PlaylistItem {
        playlist_id: id.clone(),
        uri: if id.is_empty() { String::new() } else { format!("ytmusic:playlist:{id}") },
        title: s(v, "title"),
        description: s(v, "description"),
        // get_library_playlists отдаёт count строкой («5»), get_playlist — trackCount числом
        track_count: v.get("count").and_then(Value::as_i64)
            .or_else(|| v.get("count").and_then(Value::as_str).and_then(|c| c.parse().ok()))
            .unwrap_or(
                v.get("songCount").and_then(Value::as_i64).unwrap_or(
                    v.get("trackCount").and_then(Value::as_i64).unwrap_or(0),
                ),
            ),
        cover: pick_thumbnail(&thumb, COVER_PX),
        // автор — строка (библиотека) или {name, id} (страница плейлиста)
        owner: s(v, "author").if_empty(s(author.unwrap_or(&Value::Null), "name")),
        owner_id: s(author.unwrap_or(&Value::Null), "id"),
        can_edit: v.get("owned").and_then(Value::as_bool).unwrap_or(false),
        collaborative: false,
        public: s(v, "privacy") == "PUBLIC",
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

/// Классифицировать одну запись полки главной по полям ytmusicapi:
/// videoType PODCAST → episode, videoId → track, playlistId → playlist,
/// browseId MPREb_ → album, MPSP → show, UC → artist.
fn home_item(v: &Value) -> Option<(String, serde_json::Value)> {
    let video_type = s(v, "videoType");
    if video_type.contains("PODCAST") {
        let ep = episode_item(v);
        if !ep.episode_id.is_empty() {
            return Some(("episode".to_owned(), serde_json::to_value(&ep).ok()?));
        }
    }
    if !s(v, "videoId").is_empty() {
        let t = track_item(v);
        if !t.track_id.is_empty() {
            return Some(("track".to_owned(), serde_json::to_value(&t).ok()?));
        }
    }
    if !s(v, "playlistId").is_empty() {
        let p = playlist_item(v);
        if !p.playlist_id.is_empty() {
            return Some(("playlist".to_owned(), serde_json::to_value(&p).ok()?));
        }
    }
    if let Some(bid) = v.get("browseId").and_then(Value::as_str) {
        if bid.starts_with("MPREb_") {
            let a = album_item(v);
            if !a.album_id.is_empty() {
                return Some(("album".to_owned(), serde_json::to_value(&a).ok()?));
            }
        } else if bid.starts_with("MPSP") {
            let sh = show_item(v);
            if !sh.show_id.is_empty() {
                return Some(("show".to_owned(), serde_json::to_value(&sh).ok()?));
            }
        } else if bid.starts_with("UC") {
            let ar = artist_item(v);
            if !ar.artist_id.is_empty() {
                return Some(("artist".to_owned(), serde_json::to_value(&ar).ok()?));
            }
        }
    }
    None
}

/// get_home shelves → md3-model HomeShelf items. Каждая запись полки clasificруется
/// через `home_item` (трек / плейлист / альбом / исполнитель / подкаст / выпуск),
/// а kind полки берётся из первой записи. Пустые полки пропускаются.
pub fn home_shelves(home: &[Value]) -> Vec<m::HomeShelf> {
    home.iter()
        .filter_map(|row| {
            let contents = row.get("contents").cloned().unwrap_or(Value::Array(vec![]));
            let classified: Vec<(String, serde_json::Value)> = match contents {
                Value::Array(list) => list
                    .iter()
                    .filter_map(|v| home_item(v))
                    .collect(),
                _ => vec![],
            };
            if classified.is_empty() {
                return None;
            }
            let kind = classified.first().map(|(k, _)| k.clone()).unwrap_or_default();
            let items: Vec<serde_json::Value> = classified.into_iter().map(|(_, v)| v).collect();
            let title = s(row, "title");
            Some(m::HomeShelf {
                key: title.clone(),
                title,
                kind,
                items,
            })
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
