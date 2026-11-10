//! Web API and librespot models → md3-model items for the player snapshot (our own copy of the sf_api::map rules:
//! cover art 300/640, artists joined with ", ", available = is_playable != false).
use md3_model::{ArtistRef, DeviceItem, PlayContext, TrackItem, contexts};
use sf_api::map::{COVER_LARGE_PX, COVER_PX};
use sf_api::models::{self, PlayableItem, pick_image};
use sf_api::util::{uri_id, uri_kind};

use crate::engine::LocalTrack;

fn artist_refs(artists: &[models::ArtistRef]) -> Vec<ArtistRef> {
    artists
        .iter()
        .map(|artist| ArtistRef { id: artist.id.clone().unwrap_or_default(), name: artist.name.clone() })
        .collect()
}

fn names(artists: &[models::ArtistRef]) -> String {
    models::join_names(artists.iter().map(|artist| artist.name.as_str()))
}

pub fn track(track: &models::Track) -> TrackItem {
    let album = track.album.as_ref();
    let images = album.map(|album| album.images.as_slice()).unwrap_or_default();
    TrackItem {
        track_id: track.id.clone().or_else(|| uri_id(&track.uri).map(str::to_owned)).unwrap_or_default(),
        album_id: album.map(|album| album.id.clone()).unwrap_or_default(),
        title: track.name.clone(),
        artists: names(&track.artists),
        artist_refs: artist_refs(&track.artists),
        album: album.map(|album| album.name.clone()).unwrap_or_default(),
        cover: pick_image(images, COVER_PX).unwrap_or_default().to_owned(),
        cover_large: pick_image(images, COVER_LARGE_PX).unwrap_or_default().to_owned(),
        duration_ms: i64::from(track.duration_ms),
        explicit: track.explicit,
        available: track.is_playable != Some(false) && !track.is_local,
        uri: track.uri.clone(),
        ..TrackItem::default()
    }
}

pub fn episode(episode: &models::Episode) -> TrackItem {
    let show = episode.show.as_ref();
    let pick = |px| {
        pick_image(&episode.images, px)
            .or_else(|| show.and_then(|show| pick_image(&show.images, px)))
            .unwrap_or_default()
            .to_owned()
    };
    TrackItem {
        track_id: episode.id.clone(),
        title: episode.name.clone(),
        artists: show.map(|show| show.name.clone()).unwrap_or_default(),
        album: show.map(|show| show.name.clone()).unwrap_or_default(),
        cover: pick(COVER_PX),
        cover_large: pick(COVER_LARGE_PX),
        duration_ms: i64::from(episode.duration_ms),
        explicit: episode.explicit,
        available: true,
        uri: episode.uri.clone(),
        is_episode: true,
        show_id: show.map(|show| show.id.clone()).unwrap_or_default(),
        ..TrackItem::default()
    }
}

pub fn playable(item: &PlayableItem) -> TrackItem {
    match item {
        PlayableItem::Track(item) => track(item),
        PlayableItem::Episode(item) => episode(item),
    }
}

/// A track from a librespot report; whatever is missing (the album, artist ids) — from the cache.
pub fn local_track(local: &LocalTrack, cached: Option<&TrackItem>) -> TrackItem {
    let artists: Vec<ArtistRef> = if local.artists.iter().any(|artist| artist.id.is_some()) {
        artist_refs(&local.artists)
    } else {
        cached.map(|cached| cached.artist_refs.clone()).unwrap_or_else(|| artist_refs(&local.artists))
    };
    let art_large = local.art_url.clone().unwrap_or_default();
    let art_small = local.art_small_url.clone().or_else(|| local.art_url.clone()).unwrap_or_default();
    let title = if local.title.is_empty() { cached.map(|c| c.title.clone()).unwrap_or_default() } else { local.title.clone() };
    let artist_names = if local.artists.is_empty() {
        cached.map(|c| c.artists.clone()).unwrap_or_default()
    } else {
        local.artist_names()
    };
    TrackItem {
        track_id: uri_id(&local.uri).unwrap_or_default().to_owned(),
        album_id: cached.map(|cached| cached.album_id.clone()).unwrap_or_default(),
        title,
        artists: artist_names,
        artist_refs: artists,
        album: if local.album.is_empty() { cached.map(|c| c.album.clone()).unwrap_or_default() } else { local.album.clone() },
        cover: if art_small.is_empty() { cached.map(|c| c.cover.clone()).unwrap_or_default() } else { art_small },
        cover_large: if art_large.is_empty() { cached.map(|c| c.cover_large.clone()).unwrap_or_default() } else { art_large },
        duration_ms: if local.duration_ms > 0 {
            i64::from(local.duration_ms)
        } else {
            cached.map(|c| c.duration_ms).unwrap_or_default()
        },
        explicit: cached.is_some_and(|cached| cached.explicit),
        available: true,
        uri: local.uri.clone(),
        is_episode: local.is_episode,
        show_id: cached.map(|cached| cached.show_id.clone()).unwrap_or_default(),
        ..TrackItem::default()
    }
}

/// A Connect device; `this_device_id` — this PC's id.
pub fn device(device: &models::Device, this_device_id: Option<&str>) -> DeviceItem {
    let id = device.id.clone().unwrap_or_default();
    DeviceItem {
        is_this_computer: !id.is_empty() && Some(id.as_str()) == this_device_id,
        id,
        name: device.name.clone(),
        kind: device.kind.to_lowercase(),
        is_active: device.is_active,
        is_restricted: device.is_restricted,
        supports_volume: !device.is_restricted && device.supports_volume != Some(false),
        volume: device.volume_percent.map_or(-1, i32::from),
        source: "api".to_owned(),
    }
}

/// The context type (Player.source) by its URI.
pub fn context_kind(uri: &str) -> &'static str {
    if uri.ends_with(":collection") || uri.contains(":collection:") {
        return contexts::LIKED;
    }
    match uri_kind(uri) {
        Some("playlist") | Some("user") => contexts::PLAYLIST,
        Some("album") => contexts::ALBUM,
        Some("artist") => contexts::ARTIST,
        Some("show") => contexts::SHOW,
        Some("episode") => contexts::EPISODE,
        Some("station") => contexts::RADIO,
        Some("track") => contexts::TRACKS,
        Some("search") => contexts::SEARCH,
        _ => contexts::UNKNOWN,
    }
}

/// A context known only by URI (a remote device, someone else's start).
pub fn context_from_uri(uri: &str) -> PlayContext {
    PlayContext::with_uri(context_kind(uri), uri)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_track_keeps_its_cover_sizes_and_joined_artists() {
        let item = models::Track {
            id: Some("t".into()),
            uri: "spotify:track:t".into(),
            name: "Song".into(),
            duration_ms: 1000,
            artists: vec![
                models::ArtistRef { id: Some("a".into()), name: "A".into(), uri: None },
                models::ArtistRef { id: None, name: "B".into(), uri: None },
            ],
            album: Some(models::Album {
                id: "al".into(),
                name: "Album".into(),
                images: vec![
                    models::Image { url: "big".into(), width: Some(640), height: Some(640) },
                    models::Image { url: "mid".into(), width: Some(300), height: Some(300) },
                    models::Image { url: "small".into(), width: Some(64), height: Some(64) },
                ],
                ..Default::default()
            }),
            is_playable: Some(false),
            ..Default::default()
        };
        let row = track(&item);
        assert_eq!(row.artists, "A, B");
        assert_eq!(row.cover, "mid");
        assert_eq!(row.cover_large, "big");
        assert_eq!(row.album_id, "al");
        assert!(!row.available);
        assert_eq!(row.duration_ms, 1000);
    }

    #[test]
    fn contexts_are_named_by_their_uri() {
        assert_eq!(context_kind("spotify:user:me:collection"), "liked");
        assert_eq!(context_kind("spotify:playlist:x"), "playlist");
        assert_eq!(context_kind("spotify:station:track:x"), "radio");
        assert_eq!(context_kind("spotify:album:x"), "album");
        assert_eq!(context_kind("weird"), "");
    }

    #[test]
    fn a_device_without_volume_reports_minus_one() {
        let item = device(
            &models::Device { id: Some("pc".into()), kind: "Computer".into(), ..Default::default() },
            Some("pc"),
        );
        assert!(item.is_this_computer);
        assert_eq!(item.kind, "computer");
        assert_eq!(item.volume, -1);
        assert!(item.supports_volume);
    }
}
