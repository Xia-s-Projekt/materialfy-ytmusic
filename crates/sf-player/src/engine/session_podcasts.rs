//! Podcasts through the librespot session (without the Web API quota), in the form of Web API responses.
//!
//! Verified live (2026-10):
//! - a show — spclient metadata/4/show (SHOW_V4): the header and the URIs of all episodes, newest first (for a big show
//!   ~2800 episodes and ~100 KB); the episode titles are not there — the details are fetched in EPISODE_V4 batches.
//!   SHOW_V4_BASE (the header without episodes) answers 404, so the library uses SHOW_V4 too;
//! - saved shows — the "show" collection set, "Your episodes" — "listenlater" (session_collection);
//! - the resume points are not in the metadata: the GraphQL gateway provides them (sf-services).
use std::collections::{BTreeSet, HashMap};

use librespot_core::{Session, SpotifyUri, spotify_id::SpotifyId};
use librespot_metadata::Show as SessionShow;
use librespot_metadata::image::Images;
use librespot_protocol::extended_metadata::{BatchedEntityRequest, EntityRequest, ExtensionQuery};
use librespot_protocol::extension_kind::ExtensionKind;
use protobuf::{EnumOrUnknown, Message as _};
use sf_api::ApiError;
use sf_api::models::{Episode, Image, Page, PlayableItem, Show};

use super::session_collection::{self, CollectionItem};
use super::session_reads::{self, Failure, IMAGE_HOST};

/// Episodes in a single EPISODE_V4 batch request (≈1 KB per episode).
const EPISODE_BATCH: usize = 50;
/// Shows in a single SHOW_V4 batch request: each one carries the whole episode list.
const SHOW_BATCH: usize = 20;

/// A show as /shows/{id} (without the episode page) and the URIs of all its episodes, newest first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShowOverview {
    pub show: Show,
    pub episodes: Vec<String>,
    /// An audiobook: librespot does not play it.
    pub audiobook: bool,
}

/// A show by id.
pub async fn show(session: &Session, id: &str) -> Result<ShowOverview, Failure> {
    let uri = SpotifyId::from_base62(id).map(|id| SpotifyUri::Show { id }).map_err(|_| invalid_id())?.to_uri();
    match show_batch(session, std::slice::from_ref(&uri)).await?.remove(&uri) {
        Some(Some(overview)) => Ok(overview),
        // Spotify answered that the show does not exist (404/451): the Web API would answer the same
        Some(None) => Err(not_found()),
        None => Err(Failure::Retry(anyhow::anyhow!("no metadata answer for {uri}"))),
    }
}

/// The header and a page of episodes (`offset`, no more than `limit`), like /shows/{id} and /shows/{id}/episodes.
/// An episode Spotify no longer returns is not on the page — same as with the Web API.
pub async fn show_page(
    session: &Session,
    id: &str,
    offset: u32,
    limit: u32,
) -> Result<(Show, Page<Episode>), Failure> {
    let overview = show(session, id).await?;
    let window = window(&overview.episodes, offset, limit);
    let mut items = episodes(session, window).await?;
    for episode in &mut items {
        attach_show(episode, &overview.show);
    }
    let total = overview.episodes.len() as u32;
    Ok((overview.show, page(items, total, offset, limit)))
}

/// An episode by id: with the show's id and name, without a resume point.
pub async fn episode(session: &Session, id: &str) -> Result<Episode, Failure> {
    let uri = SpotifyId::from_base62(id).map(|id| SpotifyUri::Episode { id }).map_err(|_| invalid_id())?;
    match session_reads::metadata(session, std::iter::once(&uri)).await?.remove(&uri.to_uri()) {
        Some(PlayableItem::Episode(episode)) => Ok(episode),
        // Spotify answered that the episode does not exist (404/451): the Web API would answer the same
        _ => Err(not_found()),
    }
}

/// Episodes by URI, in the same order; an episode Spotify does not return is absent. Other failures — Retry.
pub async fn episodes(session: &Session, uris: &[String]) -> Result<Vec<Episode>, Failure> {
    let parsed: Vec<SpotifyUri> = uris
        .iter()
        .filter_map(|uri| SpotifyUri::from_uri(uri).ok())
        .filter(|uri| matches!(uri, SpotifyUri::Episode { .. }))
        .collect();
    // batches in order: a page is one or two batches; a stream of batches would not let the future be Send
    let mut found: HashMap<String, PlayableItem> = HashMap::new();
    for chunk in parsed.chunks(EPISODE_BATCH) {
        found.extend(session_reads::metadata(session, chunk.iter()).await?);
    }
    Ok(uris
        .iter()
        .filter_map(|uri| match found.remove(uri) {
            Some(PlayableItem::Episode(episode)) => Some(episode),
            _ => None,
        })
        .collect())
}

/// Shows by URI in SHOW_V4 batches; a show Spotify does not return is not in the response.
pub async fn shows(session: &Session, uris: &[String]) -> Result<HashMap<String, ShowOverview>, Failure> {
    let uris: Vec<String> = uris.iter().filter(|uri| uri.starts_with("spotify:show:")).cloned().collect();
    let mut shows = HashMap::new();
    for chunk in uris.chunks(SHOW_BATCH) {
        shows.extend(show_batch(session, chunk).await?.into_iter().filter_map(|(uri, overview)| Some((uri, overview?))));
    }
    Ok(shows)
}

/// Saved shows (the "show" set), newest first, with the time added. Audiobooks are marked.
pub async fn saved_shows(session: &Session) -> Result<Vec<(CollectionItem, ShowOverview)>, Failure> {
    let items = session_collection::collection_all(session, session_collection::SET_SHOW)
        .await
        .map_err(Failure::Retry)?;
    let uris: Vec<String> = items.iter().map(|item| item.uri.clone()).collect();
    let mut found = shows(session, &uris).await?;
    Ok(items.into_iter().filter_map(|item| found.remove(&item.uri).map(|show| (item, show))).collect())
}

/// "Your episodes" (the "listenlater" set), newest first, with the time added.
pub async fn saved_episodes(session: &Session) -> Result<Vec<(CollectionItem, Episode)>, Failure> {
    let items = session_collection::collection_all(session, session_collection::SET_LISTEN_LATER)
        .await
        .map_err(Failure::Retry)?;
    let uris: Vec<String> = items.iter().map(|item| item.uri.clone()).collect();
    let mut found: HashMap<String, Episode> =
        episodes(session, &uris).await?.into_iter().map(|episode| (episode.uri.clone(), episode)).collect();
    Ok(items.into_iter().filter_map(|item| found.remove(&item.uri).map(|episode| (item, episode))).collect())
}

/// The SHOW_V4 response by URI: None — Spotify answered that the show does not exist (404/451). A URI without a response is not in the map.
async fn show_batch(session: &Session, uris: &[String]) -> Result<HashMap<String, Option<ShowOverview>>, Failure> {
    let mut request = BatchedEntityRequest::new();
    for uri in uris {
        request.entity_request.push(EntityRequest {
            entity_uri: uri.clone(),
            query: vec![ExtensionQuery {
                extension_kind: EnumOrUnknown::new(ExtensionKind::SHOW_V4),
                ..Default::default()
            }],
            ..Default::default()
        });
    }
    let response = session.spclient().get_extended_metadata(request).await?;
    let retry = |reason: String| Failure::Retry(anyhow::anyhow!(reason));
    let mut shows = HashMap::new();
    for array in response.extended_metadata {
        let provider = array.header.provider_error_status;
        if !matches!(provider, 0 | 200) {
            return Err(retry(format!("metadata provider answered {provider}")));
        }
        if array.extension_kind.enum_value() != Ok(ExtensionKind::SHOW_V4) {
            continue;
        }
        for data in array.extension_data {
            match data.header.status_code {
                0 | 200 => {}
                404 | 451 => {
                    shows.insert(data.entity_uri, None);
                    continue;
                }
                code => return Err(retry(format!("{} answered {code}", data.entity_uri))),
            }
            let overview = data
                .extension_data
                .as_ref()
                .and_then(|any| librespot_protocol::metadata::Show::parse_from_bytes(&any.value).ok())
                .and_then(|message| overview(&message))
                .ok_or_else(|| retry(format!("unreadable metadata for {}", data.entity_uri)))?;
            shows.insert(data.entity_uri, Some(overview));
        }
    }
    Ok(shows)
}

/// An SHOW_V4 record → a Web API show and the episode URIs (each once, in record order).
fn overview(message: &librespot_protocol::metadata::Show) -> Option<ShowOverview> {
    let show = SessionShow::try_from(message).ok()?;
    let mut seen = BTreeSet::new();
    let episodes: Vec<String> = show
        .episodes
        .iter()
        .map(SpotifyUri::to_uri)
        .filter(|uri| seen.insert(uri.clone()))
        .collect();
    Some(ShowOverview {
        show: Show {
            id: show.id.to_id(),
            uri: show.id.to_uri(),
            name: show.name.clone(),
            publisher: show.publisher.clone(),
            description: show.description.clone(),
            images: images(&show.covers),
            total_episodes: Some(episodes.len() as u32),
            explicit: show.is_explicit,
            ..Default::default()
        },
        episodes,
        audiobook: show.is_audiobook,
    })
}

/// An episode with no show (or no show id) gets a reference to `show`.
fn attach_show(episode: &mut Episode, show: &Show) {
    match &mut episode.show {
        Some(own) if !own.id.is_empty() => {
            if own.name.is_empty() {
                own.name = show.name.clone();
            }
        }
        _ => {
            episode.show = Some(Show {
                id: show.id.clone(),
                uri: show.uri.clone(),
                name: show.name.clone(),
                publisher: show.publisher.clone(),
                images: show.images.clone(),
                ..Default::default()
            })
        }
    }
}

/// The URIs on the `offset`/`limit` page.
fn window(uris: &[String], offset: u32, limit: u32) -> &[String] {
    let start = (offset as usize).min(uris.len());
    let end = start.saturating_add(limit as usize).min(uris.len());
    &uris[start..end]
}

fn page(items: Vec<Episode>, total: u32, offset: u32, limit: u32) -> Page<Episode> {
    let end = offset.saturating_add(limit);
    Page { items, total, limit, offset, next: (end < total).then(|| format!("offset={end}")) }
}

fn images(images: &Images) -> Vec<Image> {
    images
        .iter()
        .map(|image| Image {
            url: format!("{IMAGE_HOST}{}", image.id),
            width: u32::try_from(image.width).ok().filter(|width| *width > 0),
            height: u32::try_from(image.height).ok().filter(|height| *height > 0),
        })
        .collect()
}

fn invalid_id() -> Failure {
    Failure::Definitive(ApiError::Status { status: 400, message: "Invalid base62 id".into() })
}

fn not_found() -> Failure {
    Failure::Definitive(ApiError::Status { status: 404, message: "Not Found".into() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use librespot_protocol::metadata::{Episode as EpisodeMessage, Image as ImageMessage, ImageGroup};

    fn gid(byte: u8) -> Vec<u8> {
        vec![byte; 16]
    }

    fn message() -> librespot_protocol::metadata::Show {
        let mut message = librespot_protocol::metadata::Show::new();
        message.set_gid(gid(1));
        message.set_name("Show".into());
        message.set_publisher("Author".into());
        message.set_description("Description".into());
        message.set_explicit(true);
        let mut image = ImageMessage::new();
        image.set_file_id(vec![0xab; 20]);
        image.set_width(640);
        image.set_height(640);
        let mut group = ImageGroup::new();
        group.image.push(image);
        message.cover_image = Some(group).into();
        for byte in [2, 3, 2, 4] {
            let mut episode = EpisodeMessage::new();
            episode.set_gid(gid(byte));
            message.episode.push(episode);
        }
        message
    }

    #[test]
    fn a_show_record_reads_as_the_web_api_show_with_its_episodes_once() {
        let overview = overview(&message()).unwrap();
        let id = SpotifyId::from_raw(&gid(1)).unwrap().to_base62();
        assert_eq!(overview.show.id, id);
        assert_eq!(overview.show.uri, format!("spotify:show:{id}"));
        assert_eq!(overview.show.name, "Show");
        assert_eq!(overview.show.publisher, "Author");
        assert!(overview.show.explicit);
        assert_eq!(overview.show.images[0].url, format!("{IMAGE_HOST}{}", "ab".repeat(20)));
        assert_eq!(overview.show.images[0].width, Some(640));
        let episode = |byte| format!("spotify:episode:{}", SpotifyId::from_raw(&gid(byte)).unwrap().to_base62());
        assert_eq!(overview.episodes, vec![episode(2), episode(3), episode(4)], "order kept, repeats dropped");
        assert_eq!(overview.show.total_episodes, Some(3));
        assert!(!overview.audiobook);
    }

    #[test]
    fn pages_cut_the_episode_list_like_the_web_api() {
        let uris: Vec<String> = (0..5).map(|n| n.to_string()).collect();
        assert_eq!(window(&uris, 0, 2), ["0", "1"]);
        assert_eq!(window(&uris, 4, 50), ["4"]);
        assert!(window(&uris, 9, 50).is_empty());
        assert!(page(Vec::new(), 5, 0, 2).next_offset().is_some());
        assert_eq!(page(Vec::new(), 5, 4, 2).next_offset(), None);
    }

    #[test]
    fn an_episode_without_its_show_links_to_the_page_show() {
        let show = Show { id: "s".into(), uri: "spotify:show:s".into(), name: "Show".into(), ..Default::default() };
        let mut bare = Episode::default();
        attach_show(&mut bare, &show);
        assert_eq!(bare.show.as_ref().map(|own| own.id.as_str()), Some("s"));
        let mut named = Episode {
            show: Some(Show { id: "x".into(), name: String::new(), ..Default::default() }),
            ..Default::default()
        };
        attach_show(&mut named, &show);
        let own = named.show.unwrap();
        assert_eq!((own.id.as_str(), own.name.as_str()), ("x", "Show"));
    }
}
