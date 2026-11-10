//! "New releases" through the librespot session: the personal "Release Radar", artist subscriptions and their releases with dates.
//! Web API here is only a fallback path: the shared key runs into 429, and what is needed is available to Spotify clients.
//!
//! "Radar" (Release Radar): since November 2024 Web API does not give algorithmic
//! Spotify playlists to applications in development mode (404), and each account has its own id.
//! Here it is looked up on the home of the mobile client (spclient homeview); sf-services asks
//! for the "For You" page (GraphQL). The playlist itself is then read through spclient,
//! like any other (session_reads).
//!
//! Subscriptions — the "artist" set of the collection (spclient /collection/v2/paging, protobuf collection2v2: librespot does not
//! build it, the PageRequest/PageResponse fields are written and read here by hand). Releases — the artist record
//! (albums and singles, without dates) and ALBUM_V4 extended-metadata batches with dates, as in session_reads::artist.
use std::collections::HashMap;

use librespot_core::{Session, SpotifyUri, spotify_id::SpotifyId};
use librespot_metadata::Album as SessionAlbum;
use librespot_metadata::artist::Artist as SessionArtist;
use librespot_metadata::image::Images;
use librespot_protocol::extended_metadata::{BatchedEntityRequest, EntityRequest, ExtensionQuery};
use librespot_protocol::extension_kind::ExtensionKind;
use protobuf::{EnumOrUnknown, Message as _};
use sf_api::models::{Album, ArtistRef, CategoryPlaylists, Image, Page, Playlist};

/// Browse category "Made For You": personal Discover Weekly, Release Radar, Daily Mix, daylist.
pub const MADE_FOR_YOU_CATEGORY: &str = "0JQ5DAt0tbjZptfcdMSKl3";
/// Playlists in one category answer (Web API gives no more; personal ones are usually 10–20).
pub const CATEGORY_LIMIT: u32 = 50;
/// Releases in one batched extended-metadata request.
pub const RELEASE_BATCH: usize = 50;

const IMAGE_HOST: &str = "https://i.scdn.co/image/";
/// Releases of a group that are read at all (as in session_reads: records list them not by date).
const RELEASES_PER_GROUP: usize = 200;
const COLLECTION_PAGE: i32 = 300;
/// Collection body type: on application/x-protobuf the service answers 400 (verified on a live account).
const COLLECTION_TYPE: &str = "application/vnd.collection-v2.spotify.proto";
const COLLECTION_PAGES: usize = 10;

// --- Release Radar ---------------------------------------------------------------------------------

/// "Radar" names in Spotify localizations (case-insensitive); anything else is recognized by the word "radar".
const RADAR_NAMES: &[&str] = &[
    "release radar",
    "радар новинок",
    "radar de novedades",
    "radar des sorties",
    "radar de novidades",
    "radar premier",
];

/// Spotify playlist ids ("37i9dQZ…"): personal algorithmic and charts — "37i9dQZEVXb…" (Discover Weekly has
/// "…EVXc…"), editorial — "37i9dQZF1…" (among them is "RADAR India" — that is not the "Release Radar").
const SPOTIFY_MADE: &str = "37i9dQZ";
const PERSONAL: &str = "37i9dQZEVXb";

/// A playlist is the personal "Release Radar": Spotify keeps it (the owner is unknown — judged by id), and the name is
/// one of the localized ones; in another language — the word "radar" on a personal playlist.
pub fn is_release_radar(id: &str, name: &str, owner_id: &str) -> bool {
    let by_spotify = owner_id == "spotify" || (owner_id.is_empty() && id.starts_with(SPOTIFY_MADE));
    if !by_spotify {
        return false;
    }
    let name = name.trim().to_lowercase();
    RADAR_NAMES.contains(&name.as_str())
        || (id.starts_with(PERSONAL) && (name.contains("radar") || name.contains("радар")))
}

/// "Radar" among playlists: first the exact name, then a personal one with the word "radar".
pub fn pick_release_radar(playlists: &[Playlist]) -> Option<&Playlist> {
    let owner = |p: &Playlist| p.owner.id.clone().unwrap_or_default();
    let fits = |p: &&Playlist| !p.id.is_empty() && is_release_radar(&p.id, &p.name, &owner(p));
    let exact = playlists
        .iter()
        .filter(fits)
        .find(|p| RADAR_NAMES.contains(&p.name.trim().to_lowercase().as_str()));
    exact.or_else(|| playlists.iter().find(fits))
}

/// Playlists of the Spotify client's home (spclient /homeview/v1/home — JSON hubs): a card has target.uri,
/// text.title/subtitle, images.main.uri. The card has no owner — Spotify, if the id is its own.
pub fn parse_home(bytes: &[u8]) -> anyhow::Result<Vec<Playlist>> {
    fn text(map: &serde_json::Map<String, serde_json::Value>, key: &str) -> String {
        map.get("text")
            .and_then(|t| t.get(key))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned()
    }
    fn walk(node: &serde_json::Value, out: &mut Vec<Playlist>) {
        match node {
            serde_json::Value::Object(map) => {
                let uri = map
                    .get("target")
                    .and_then(|t| t.get("uri"))
                    .and_then(|u| u.as_str())
                    .unwrap_or("");
                if let Some(id) = uri.strip_prefix("spotify:playlist:")
                    && !out.iter().any(|p| p.id == id)
                {
                    let image = map
                        .get("images")
                        .and_then(|i| i.get("main"))
                        .and_then(|m| m.get("uri"))
                        .and_then(|u| u.as_str())
                        .filter(|u| u.starts_with("http"));
                    let mut playlist = Playlist {
                        id: id.to_owned(),
                        uri: uri.to_owned(),
                        name: text(map, "title"),
                        description: Some(text(map, "subtitle")).filter(|d| !d.is_empty()),
                        images: image
                            .map(|url| {
                                vec![Image {
                                    url: url.to_owned(),
                                    width: None,
                                    height: None,
                                }]
                            })
                            .unwrap_or_default(),
                        ..Default::default()
                    };
                    if id.starts_with(SPOTIFY_MADE) {
                        playlist.owner.id = Some("spotify".to_owned());
                        playlist.owner.display_name = Some("Spotify".to_owned());
                    }
                    out.push(playlist);
                }
                map.values().for_each(|v| walk(v, out));
            }
            serde_json::Value::Array(items) => items.iter().for_each(|v| walk(v, out)),
            _ => {}
        }
    }
    let json: serde_json::Value = serde_json::from_slice(bytes)?;
    let mut out = Vec::new();
    walk(&json, &mut out);
    Ok(out)
}

/// Playlists of the session account's home — as the mobile Spotify client sees it (the "Only For You" shelf).
pub async fn home_playlists(session: &Session) -> anyhow::Result<Vec<Playlist>> {
    let bytes = session
        .spclient()
        .request_as_json(
            &reqwest::Method::GET,
            "/homeview/v1/home?platform=android",
            None,
            None,
        )
        .await?;
    parse_home(&bytes)
}

/// /browse/categories/{id}/playlists answer → playlists (Spotify's empty spots — null — are skipped).
pub fn parse_category(bytes: &[u8]) -> anyhow::Result<Vec<Playlist>> {
    let category: CategoryPlaylists = serde_json::from_slice(bytes)?;
    let Page { items, .. } = category.playlists;
    Ok(items.into_iter().filter(|p| !p.id.is_empty()).collect())
}

/// The session account's personal "Release Radar" from the mobile Spotify client's home (spclient homeview, without the
/// Web API quota). Ok(None) — the home answered, but there is no "Radar" on it. sf-services asks for the "For You" page (GraphQL
/// browsePage) (data/home_feed); the api.spotify.com category with the session token is no longer needed — it
/// spent the Spotify client's Web API quota and often answered 429 QUOTA_EXCEEDED.
pub async fn release_radar(session: &Session) -> anyhow::Result<Option<Playlist>> {
    let home = home_playlists(session).await?;
    Ok(pick_release_radar(&home).cloned())
}

// --- subscriptions (collection2v2) ------------------------------------------------------------------------

fn put_varint(buf: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        buf.push((value as u8) | 0x80);
        value >>= 7;
    }
    buf.push(value as u8);
}

fn put_text(buf: &mut Vec<u8>, field: u32, text: &str) {
    if text.is_empty() {
        return;
    }
    put_varint(buf, u64::from(field << 3 | 2));
    put_varint(buf, text.len() as u64);
    buf.extend_from_slice(text.as_bytes());
}

/// PageRequest { username = 1; set = 2; pagination_token = 3; limit = 4 }.
fn page_request(username: &str, set: &str, token: &str, limit: i32) -> Vec<u8> {
    let mut buf = Vec::new();
    put_text(&mut buf, 1, username);
    put_text(&mut buf, 2, set);
    put_text(&mut buf, 3, token);
    put_varint(&mut buf, 4 << 3);
    put_varint(&mut buf, limit.max(0) as u64);
    buf
}

/// Protobuf field: number and value (varint or bytes; fixed32/64 are skipped as bytes).
enum Value<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
}

struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    fn varint(&mut self) -> Option<u64> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let (&byte, rest) = self.bytes.split_first()?;
            self.bytes = rest;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Some(value);
            }
        }
        None
    }

    fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        if self.bytes.len() < len {
            return None;
        }
        let (head, rest) = self.bytes.split_at(len);
        self.bytes = rest;
        Some(head)
    }

    /// Next field; None — end (or corrupt data: nothing further is read).
    fn field(&mut self) -> Option<(u32, Value<'a>)> {
        if self.bytes.is_empty() {
            return None;
        }
        let tag = self.varint()?;
        let number = u32::try_from(tag >> 3).ok()?;
        let value = match tag & 7 {
            0 => Value::Varint(self.varint()?),
            1 => Value::Bytes(self.take(8)?),
            2 => {
                let len = usize::try_from(self.varint()?).ok()?;
                Value::Bytes(self.take(len)?)
            }
            5 => Value::Bytes(self.take(4)?),
            _ => return None,
        };
        Some((number, value))
    }
}

/// Collection page: live URIs (is_removed = false) and the next page token ("" — last).
/// PageResponse { repeated CollectionItem items = 1; next_page_token = 2 },
/// CollectionItem { uri = 1; added_at = 2; is_removed = 3 }.
pub fn parse_collection_page(bytes: &[u8]) -> (Vec<String>, String) {
    let mut uris = Vec::new();
    let mut next = String::new();
    let mut reader = Reader { bytes };
    while let Some((number, value)) = reader.field() {
        match (number, value) {
            (1, Value::Bytes(item)) => {
                let mut uri = String::new();
                let mut removed = false;
                let mut fields = Reader { bytes: item };
                while let Some((number, value)) = fields.field() {
                    match (number, value) {
                        (1, Value::Bytes(text)) => uri = String::from_utf8_lossy(text).into_owned(),
                        (3, Value::Varint(flag)) => removed = flag != 0,
                        _ => {}
                    }
                }
                if !uri.is_empty() && !removed {
                    uris.push(uri);
                }
            }
            (2, Value::Bytes(text)) => next = String::from_utf8_lossy(text).into_owned(),
            _ => {}
        }
    }
    (uris, next)
}

/// ids of the artists the session account is subscribed to, in collection order.
pub async fn followed_artists(session: &Session) -> anyhow::Result<Vec<String>> {
    use reqwest::header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderValue};
    let username = session.username();
    let mut ids = Vec::new();
    let mut token = String::new();
    for _ in 0..COLLECTION_PAGES {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static(COLLECTION_TYPE));
        headers.insert(ACCEPT, HeaderValue::from_static(COLLECTION_TYPE));
        let body = page_request(&username, "artist", &token, COLLECTION_PAGE);
        let bytes = session
            .spclient()
            .request(
                &reqwest::Method::POST,
                "/collection/v2/paging",
                Some(headers),
                Some(&body),
            )
            .await?;
        let (uris, next) = parse_collection_page(&bytes);
        ids.extend(
            uris.iter()
                .filter_map(|uri| uri.strip_prefix("spotify:artist:"))
                .map(str::to_owned),
        );
        if next.is_empty() || next == token {
            break;
        }
        token = next;
    }
    Ok(ids)
}

// --- artist releases ----------------------------------------------------------------------------

/// Release from the artist record: album id and group (album | single), still without a date.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseRef {
    pub album_id: String,
    pub group: &'static str,
}

/// The artist's albums and singles by their record (the current edition of each, ≤ 200 per group).
pub async fn artist_releases(
    session: &Session,
    artist_id: &str,
) -> anyhow::Result<Vec<ReleaseRef>> {
    let uri = SpotifyUri::Artist {
        id: SpotifyId::from_base62(artist_id)?,
    };
    let bytes = session.spclient().get_artist_metadata(&uri).await?;
    let message = librespot_protocol::metadata::Artist::parse_from_bytes(&bytes)?;
    let record = SessionArtist::try_from(&message)?;
    let mut refs = Vec::new();
    for (albums, group) in [(&record.albums, "album"), (&record.singles, "single")] {
        refs.extend(
            albums
                .current_releases()
                .take(RELEASES_PER_GROUP)
                .map(|uri| ReleaseRef {
                    album_id: uri.to_id(),
                    group,
                }),
        );
    }
    Ok(refs)
}

/// Release details in one batch (≤ RELEASE_BATCH): Album as /artists/{id}/albums returns it — with
/// release_date, album_type and album_group. A release Spotify does not return (404, 451) is skipped.
pub async fn release_details(session: &Session, refs: &[ReleaseRef]) -> anyhow::Result<Vec<Album>> {
    let mut request = BatchedEntityRequest::new();
    let mut groups = HashMap::new();
    for release in refs {
        let Ok(id) = SpotifyId::from_base62(&release.album_id) else {
            continue;
        };
        let uri = SpotifyUri::Album { id }.to_uri();
        groups.insert(uri.clone(), release.group);
        request.entity_request.push(EntityRequest {
            entity_uri: uri,
            query: vec![ExtensionQuery {
                extension_kind: EnumOrUnknown::new(ExtensionKind::ALBUM_V4),
                ..Default::default()
            }],
            ..Default::default()
        });
    }
    if request.entity_request.is_empty() {
        return Ok(Vec::new());
    }
    let response = session.spclient().get_extended_metadata(request).await?;
    let mut albums = Vec::new();
    for array in response.extended_metadata {
        let provider = array.header.provider_error_status;
        anyhow::ensure!(
            matches!(provider, 0 | 200),
            "metadata provider answered {provider}"
        );
        if array.extension_kind.enum_value() != Ok(ExtensionKind::ALBUM_V4) {
            continue;
        }
        for data in array.extension_data {
            if !matches!(data.header.status_code, 0 | 200) {
                continue;
            }
            let group = groups.get(&data.entity_uri).copied().unwrap_or("album");
            let album = data.extension_data.as_ref().and_then(|any| {
                let message =
                    librespot_protocol::metadata::Album::parse_from_bytes(&any.value).ok()?;
                release(&message, group)
            });
            albums.extend(album);
        }
    }
    Ok(albums)
}

fn images(images: &Images) -> Vec<Image> {
    images
        .iter()
        .map(|image| Image {
            url: format!("{IMAGE_HOST}{}", image.id),
            width: u32::try_from(image.width).ok().filter(|width| *width > 0),
            height: u32::try_from(image.height)
                .ok()
                .filter(|height| *height > 0),
        })
        .collect()
}

/// Release as /artists/{id}/albums lists it.
fn release(message: &librespot_protocol::metadata::Album, group: &str) -> Option<Album> {
    use librespot_protocol::metadata::album::Type;
    let typed = SessionAlbum::try_from(message).ok()?;
    let kind = match typed.album_type {
        Type::SINGLE => "single",
        Type::EP => "ep",
        Type::COMPILATION => "compilation",
        _ => "album",
    };
    let tracks: usize = typed.discs.iter().map(|disc| disc.tracks.len()).sum();
    let covers = if typed.covers.is_empty() {
        &typed.cover_group
    } else {
        &typed.covers
    };
    Some(Album {
        id: typed.id.to_id(),
        uri: typed.id.to_uri(),
        name: typed.name.clone(),
        images: images(covers),
        artists: typed
            .artists
            .iter()
            .map(|artist| ArtistRef {
                id: Some(artist.id.to_id()),
                name: artist.name.clone(),
                uri: Some(artist.id.to_uri()),
            })
            .collect(),
        album_type: Some(kind.to_owned()),
        album_group: Some(group.to_owned()),
        total_tracks: u32::try_from(tracks).ok().filter(|count| *count > 0),
        release_date: message.date.as_ref().and_then(release_date),
        ..Default::default()
    })
}

/// Date with the precision Spotify knows, as Web API's release_date: "2021", "2021-11" or "2021-11-05".
fn release_date(date: &librespot_protocol::metadata::Date) -> Option<String> {
    if date.year() <= 0 {
        return None;
    }
    Some(match (date.has_month(), date.has_day()) {
        (true, true) => format!("{:04}-{:02}-{:02}", date.year(), date.month(), date.day()),
        (true, false) => format!("{:04}-{:02}", date.year(), date.month()),
        _ => format!("{:04}", date.year()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CATEGORY: &str = r#"{
        "playlists": {
            "href": "https://api.spotify.com/v1/browse/categories/0JQ5DAt0tbjZptfcdMSKl3/playlists",
            "items": [
                {"id": "37i9dQZEVXcJZyENOWUFo7", "name": "Discover Weekly", "uri": "spotify:playlist:37i9dQZEVXcJZyENOWUFo7",
                 "owner": {"id": "spotify", "display_name": "Spotify"}, "images": [{"url": "https://i.scdn.co/dw", "width": 640, "height": 640}],
                 "tracks": {"total": 30}},
                null,
                {"id": "37i9dQZF1E39vTG3GurFPW", "name": "Daily Mix 1", "uri": "spotify:playlist:37i9dQZF1E39vTG3GurFPW",
                 "owner": {"id": "spotify", "display_name": "Spotify"}, "images": []},
                {"id": "37i9dQZEVXbsGPw6v9FjFR", "name": "Release Radar", "uri": "spotify:playlist:37i9dQZEVXbsGPw6v9FjFR",
                 "description": "New releases from artists you listen to", "owner": {"id": "spotify", "display_name": "Spotify"},
                 "images": [{"url": "https://i.scdn.co/rr", "width": 640, "height": 640}], "tracks": {"total": 30}}
            ],
            "limit": 50, "offset": 0, "total": 4, "next": null
        }
    }"#;

    fn playlist(id: &str, name: &str, owner: &str) -> Playlist {
        let mut playlist = Playlist {
            id: id.into(),
            name: name.into(),
            ..Default::default()
        };
        playlist.owner.id = Some(owner.into());
        playlist
    }

    #[test]
    fn the_category_answer_reads_without_its_empty_places() {
        let playlists = parse_category(CATEGORY.as_bytes()).unwrap();
        assert_eq!(playlists.len(), 3, "null skipped");
        let radar = pick_release_radar(&playlists).unwrap();
        assert_eq!(radar.id, "37i9dQZEVXbsGPw6v9FjFR");
        assert_eq!(radar.images[0].url, "https://i.scdn.co/rr");
        assert_eq!(radar.track_total(), 30);
    }

    #[test]
    fn an_empty_or_odd_category_has_no_radar() {
        assert!(
            parse_category(br#"{"playlists": {"items": []}}"#)
                .unwrap()
                .is_empty()
        );
        assert!(parse_category(br#"{}"#).unwrap().is_empty());
        assert!(parse_category(b"not json").is_err());
        let mixes = [
            playlist("a", "Daily Mix 1", "spotify"),
            playlist("b", "Discover Weekly", "spotify"),
        ];
        assert!(pick_release_radar(&mixes).is_none());
    }

    #[test]
    fn only_spotify_keeps_the_radar() {
        let rr = "37i9dQZEVXbsGPw6v9FjFR";
        assert!(is_release_radar(rr, "Release Radar", "spotify"));
        assert!(is_release_radar(rr, "  радар НОВИНОК ", "spotify"));
        assert!(
            is_release_radar(rr, "Radar de Novedades", ""),
            "owner unknown: Spotify's id"
        );
        assert!(
            is_release_radar(rr, "Yeni Çıkanlar Radarı", "spotify"),
            "other languages by the word"
        );
        assert!(
            !is_release_radar(rr, "Release Radar", "someone"),
            "a copy someone saved"
        );
        assert!(
            !is_release_radar("1zEt4hmkqzyc1qCPOrtBGC", "Release Radar", ""),
            "not Spotify's"
        );
        assert!(!is_release_radar(rr, "Discover Weekly", "spotify"));
        assert!(
            !is_release_radar("37i9dQZF1DWTAtTdFMiJYK", "RADAR India", "spotify"),
            "an editorial playlist is not the personal radar"
        );
    }

    #[test]
    fn an_exact_name_wins_over_a_lookalike() {
        let lists = [
            playlist("37i9dQZEVXbAAAAAAAAAAA", "Radar Mix", "spotify"),
            playlist("37i9dQZEVXbBBBBBBBBBBB", "Release Radar", "spotify"),
        ];
        assert_eq!(
            pick_release_radar(&lists).unwrap().id,
            "37i9dQZEVXbBBBBBBBBBBB"
        );
        let only_lookalike = [
            playlist("", "Radar", "spotify"),
            playlist("37i9dQZEVXbCCCCCCCCCCC", "Radar Mix", "spotify"),
        ];
        assert_eq!(
            pick_release_radar(&only_lookalike).unwrap().id,
            "37i9dQZEVXbCCCCCCCCCCC"
        );
    }

    /// Shortened live /homeview/v1/home answer (the full one is 130 KB of the same shape) + the "Radar" card.
    const HOME: &str = r#"{"id":"legacy-home-page","body":[
        {"id":"spotify:section:0JQ5DAUnp4wcj0bCb3wh3S-header","component":{"id":"glue:sectionHeader"},
         "text":{"title":"Made For fatyzzz"}},
        {"id":"spotify:section:0JQ5DAUnp4wcj0bCb3wh3S_card0","component":{"id":"glue2:card"},
         "text":{"title":"Discover Weekly","subtitle":"Your shortcut to hidden gems"},
         "images":{"main":{"uri":"https://pickasso.spotifycdn.com/image/dw","placeholder":"playlist"}},
         "target":{"uri":"spotify:playlist:37i9dQZEVXcPdivXIgWGoc"}},
        {"id":"spotify:section:0JQ5DAUnp4wcj0bCb3wh3S_card1","component":{"id":"glue2:card"},
         "text":{"title":"Release Radar","subtitle":"Catch all the latest music"},
         "images":{"main":{"uri":"https://pickasso.spotifycdn.com/image/rr"}},
         "target":{"uri":"spotify:playlist:37i9dQZEVXbsGPw6v9FjFR"}},
        {"id":"shelf","children":[
            {"text":{"title":"RADAR India","subtitle":"Indie"},"target":{"uri":"spotify:playlist:37i9dQZF1DWTAtTdFMiJYK"}},
            {"text":{"title":"Mud","subtitle":"Remi Wolf"},"target":{"uri":"spotify:album:19cvoEE5d3pMkxLFL5TsAG"}},
            {"text":{"title":"Discover Weekly"},"target":{"uri":"spotify:playlist:37i9dQZEVXcPdivXIgWGoc"}}
        ]}
    ]}"#;

    #[test]
    fn the_home_view_lists_its_playlists_once() {
        let playlists = parse_home(HOME.as_bytes()).unwrap();
        let ids: Vec<&str> = playlists.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "37i9dQZEVXcPdivXIgWGoc",
                "37i9dQZEVXbsGPw6v9FjFR",
                "37i9dQZF1DWTAtTdFMiJYK"
            ]
        );
        let radar = pick_release_radar(&playlists).unwrap();
        assert_eq!(radar.id, "37i9dQZEVXbsGPw6v9FjFR");
        assert_eq!(radar.owner.id.as_deref(), Some("spotify"));
        assert_eq!(
            radar.images[0].url,
            "https://pickasso.spotifycdn.com/image/rr"
        );
        assert_eq!(
            radar.description.as_deref(),
            Some("Catch all the latest music")
        );
        // a home without the "Radar" (as a live account has now): Discover Weekly and the editorial RADAR India
        let without: Vec<Playlist> = playlists
            .into_iter()
            .filter(|p| p.name != "Release Radar")
            .collect();
        assert!(pick_release_radar(&without).is_none());
        assert!(parse_home(b"[]").unwrap().is_empty());
    }

    #[test]
    fn a_page_request_is_written_as_collection2v2_expects() {
        let bytes = page_request("alice", "artist", "", 300);
        // 1: "alice", 2: "artist", 4: 300 (varint 0xac 0x02); an empty token is not written
        let expected = [
            &[0x0a, 5][..],
            b"alice",
            &[0x12, 6],
            b"artist",
            &[0x20, 0xac, 0x02],
        ]
        .concat();
        assert_eq!(bytes, expected);
        let with_token = page_request("a", "artist", "t1", 1);
        assert!(with_token.windows(4).any(|w| w == [0x1a, 2, b't', b'1']));
    }

    fn item(uri: &str, added_at: u64, removed: bool) -> Vec<u8> {
        let mut buf = Vec::new();
        put_text(&mut buf, 1, uri);
        put_varint(&mut buf, 2 << 3);
        put_varint(&mut buf, added_at);
        if removed {
            put_varint(&mut buf, 3 << 3);
            put_varint(&mut buf, 1);
        }
        buf
    }

    #[test]
    fn a_collection_page_lists_live_artists_and_the_next_token() {
        let mut page = Vec::new();
        for entry in [
            item(
                "spotify:artist:4Z8W4fKeB5YxbusRsdQVPb",
                1_700_000_000,
                false,
            ),
            item("spotify:artist:gone", 1_700_000_001, true),
            item(
                "spotify:artist:0OdUWJ0sBjDrqHygGUXeCF",
                1_700_000_002,
                false,
            ),
        ] {
            put_varint(&mut page, 1 << 3 | 2);
            put_varint(&mut page, entry.len() as u64);
            page.extend(entry);
        }
        put_text(&mut page, 2, "next-page");
        put_text(&mut page, 3, "sync");
        let (uris, next) = parse_collection_page(&page);
        assert_eq!(
            uris,
            [
                "spotify:artist:4Z8W4fKeB5YxbusRsdQVPb",
                "spotify:artist:0OdUWJ0sBjDrqHygGUXeCF"
            ]
        );
        assert_eq!(next, "next-page");
    }

    #[test]
    fn a_broken_collection_page_reads_what_it_can() {
        assert_eq!(parse_collection_page(&[]), (Vec::new(), String::new()));
        let mut page = Vec::new();
        put_text(&mut page, 2, "t");
        page.extend([0x0a, 50, 1, 2]); // field 1 promises 50 bytes, but there are 2
        let (uris, next) = parse_collection_page(&page);
        assert!(uris.is_empty());
        assert_eq!(next, "t");
    }

    #[test]
    fn a_release_reads_with_its_group_and_date() {
        use librespot_protocol::metadata::{
            Album as AlbumMessage, Date, Disc, Track as TrackMessage, album::Type,
        };
        let mut message = AlbumMessage::new();
        message.set_gid(vec![7; 16]);
        message.set_name("Fresh".into());
        message.set_type(Type::SINGLE);
        let mut date = Date::new();
        date.set_year(2026);
        date.set_month(10);
        date.set_day(3);
        message.date = Some(date).into();
        let mut disc = Disc::new();
        let mut track = TrackMessage::new();
        track.set_gid(vec![8; 16]);
        disc.track.push(track);
        message.disc.push(disc);
        let album = release(&message, "single").unwrap();
        assert_eq!(album.name, "Fresh");
        assert_eq!(album.uri, format!("spotify:album:{}", album.id));
        assert_eq!(album.album_type.as_deref(), Some("single"));
        assert_eq!(album.album_group.as_deref(), Some("single"));
        assert_eq!(album.release_date.as_deref(), Some("2026-10-03"));
        assert_eq!(album.total_tracks, Some(1));
    }
}
