//! Podcasts through a librespot session (sf_player::engine::session_podcasts) with resume points from the GraphQL gateway.
//! Session metadata does not carry resume points, while the Web API returns them — pathfinder provides them: queryPodcastEpisodes
//! (a show's episode page) and getEpisodeOrChapter (one episode). Verified live (2026-10): an episode
//! `playedState { playPositionMilliseconds, state: NOT_STARTED | IN_PROGRESS | COMPLETED }`; decorateContext…
//! does not return it. The resume point is decoration: the gateway did not answer within PLAYED_GRACE — episodes without progress.
use std::collections::HashMap;
use std::future::Future;
use std::time::Duration;

use serde_json::{Value, json};
use sf_api::Http;
use sf_api::models::{Episode, Page, ResumePoint, Show};
use sf_player::engine::session_podcasts::{self, ShowOverview};
use sf_player::engine::session_reads::Failure;

use super::persisted;

/// queryPodcastEpisodes and getEpisodeOrChapter hashes from the web player bundle (2026-10); persisted will find fresh ones.
const PODCAST_EPISODES_HASHES: [&str; 1] = ["3539d746cf882f3909660de40b4ef472b3f5893a0761d617c282541de5d412c5"];
const EPISODE_HASHES: [&str; 1] = ["5f77db47e5a2ce6680330bb61317aa09e17ddd1d189513cb218e7cd3e43288be"];
/// How long a page waits for a resume point on top of its own metadata.
const PLAYED_GRACE: Duration = Duration::from_secs(4);
/// Neighbouring episodes on an episode page.
const MORE_EPISODES: usize = 3;

/// A show page: header and episodes from `offset` (no more than `limit`) with resume points.
pub async fn show_page(
    http: &Http,
    session: &sf_player::Session,
    show_id: &str,
    offset: u32,
    limit: u32,
) -> Result<(Show, Page<Episode>), Failure> {
    let uri = format!("spotify:show:{show_id}");
    let (page, played) = tokio::join!(
        session_podcasts::show_page(session, show_id, offset, limit),
        decoration(show_played(http, session, &uri, offset, limit)),
    );
    let (show, mut page) = page?;
    apply(&mut page.items, &played);
    Ok((show, page))
}

/// An episode (with a resume point), its show and up to three neighbouring episodes. A show that could not be read — from
/// the link in the episode itself, without neighbours: the episode page does not crash because of it.
pub async fn episode_page(
    http: &Http,
    session: &sf_player::Session,
    episode_id: &str,
) -> Result<(Episode, Show, Vec<Episode>), Failure> {
    let uri = format!("spotify:episode:{episode_id}");
    let (episode, played) =
        tokio::join!(session_podcasts::episode(session, episode_id), decoration(episode_played(http, session, &uri)));
    let mut episode = episode?;
    episode.resume_point = played.get(&uri).cloned();
    let own_show = episode.show.clone().unwrap_or_default();
    if own_show.id.is_empty() {
        return Ok((episode, own_show, Vec::new()));
    }
    let neighbours = async {
        let overview = session_podcasts::show(session, &own_show.id).await?;
        let uris: Vec<String> = overview.episodes.iter().filter(|other| **other != uri).take(MORE_EPISODES).cloned().collect();
        let more = session_podcasts::episodes(session, &uris).await?;
        Ok::<_, Failure>((overview, more))
    };
    let (neighbours, played) =
        tokio::join!(neighbours, decoration(show_played(http, session, &own_show.uri, 0, MORE_EPISODES as u32 + 1)));
    match neighbours {
        Ok((ShowOverview { show, .. }, mut more)) => {
            apply(&mut more, &played);
            Ok((episode, show, more))
        }
        Err(error) => {
            tracing::debug!("show of episode {episode_id} via session: {}", failure_text(&error));
            Ok((episode, own_show, Vec::new()))
        }
    }
}

/// Saved shows: the time added and the show; audiobooks (librespot does not play them) are skipped.
pub async fn saved_shows(session: &sf_player::Session) -> anyhow::Result<Vec<(String, Show)>> {
    let saved = session_podcasts::saved_shows(session).await.map_err(into_anyhow)?;
    Ok(saved
        .into_iter()
        .filter(|(_, overview)| !overview.audiobook)
        .map(|(item, overview)| (item.added_at_iso().unwrap_or_default(), overview.show))
        .collect())
}

/// "Your episodes", newest first (without resume points: the list does not show them).
pub async fn saved_episodes(session: &sf_player::Session) -> anyhow::Result<Vec<Episode>> {
    let saved = session_podcasts::saved_episodes(session).await.map_err(into_anyhow)?;
    Ok(saved.into_iter().map(|(_, episode)| episode).collect())
}

/// Episode resume points on a show page, by episode URI.
async fn show_played(
    http: &Http,
    session: &sf_player::Session,
    show_uri: &str,
    offset: u32,
    limit: u32,
) -> anyhow::Result<HashMap<String, ResumePoint>> {
    let variables = json!({ "uri": show_uri, "offset": offset, "limit": limit, "includeEpisodeContentRatingsV2": false });
    let body = persisted::query(http, session, "queryPodcastEpisodes", &PODCAST_EPISODES_HASHES, &variables).await?;
    Ok(played_on_page(&body))
}

/// The resume point of one episode (by URI; not started — still a place, with zero).
async fn episode_played(
    http: &Http,
    session: &sf_player::Session,
    episode_uri: &str,
) -> anyhow::Result<HashMap<String, ResumePoint>> {
    let variables = json!({ "uri": episode_uri, "includeEpisodeContentRatingsV2": false });
    let body = persisted::query(http, session, "getEpisodeOrChapter", &EPISODE_HASHES, &variables).await?;
    let episode = body.pointer("/data/episodeUnionV2").unwrap_or(&Value::Null);
    Ok(played_of(episode).map(|point| (episode_uri.to_owned(), point)).into_iter().collect())
}

/// Decoration no longer than PLAYED_GRACE; an error or being late — empty.
async fn decoration(read: impl Future<Output = anyhow::Result<HashMap<String, ResumePoint>>>) -> HashMap<String, ResumePoint> {
    match tokio::time::timeout(PLAYED_GRACE, read).await {
        Ok(Ok(played)) => played,
        Ok(Err(error)) => {
            tracing::debug!("episode resume points: {error:#}");
            HashMap::new()
        }
        Err(_) => {
            tracing::debug!("episode resume points timed out");
            HashMap::new()
        }
    }
}

/// data.podcastUnionV2.episodesV2.items[].entity.data → resume points by URI.
fn played_on_page(body: &Value) -> HashMap<String, ResumePoint> {
    body.pointer("/data/podcastUnionV2/episodesV2/items")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let episode = item.pointer("/entity/data")?;
            let uri = episode
                .get("uri")
                .or_else(|| item.pointer("/entity/_uri"))
                .and_then(Value::as_str)
                .filter(|uri| !uri.is_empty())?;
            Some((uri.to_owned(), played_of(episode)?))
        })
        .collect()
}

/// An episode's playedState as the Web API resume_point.
fn played_of(episode: &Value) -> Option<ResumePoint> {
    if episode.get("__typename").and_then(Value::as_str).is_some_and(|kind| kind != "Episode") {
        return None;
    }
    let state = episode.get("playedState")?;
    let position = state.get("playPositionMilliseconds").and_then(Value::as_u64).unwrap_or_default();
    Some(ResumePoint {
        fully_played: state.get("state").and_then(Value::as_str) == Some("COMPLETED"),
        resume_position_ms: u32::try_from(position).unwrap_or(u32::MAX),
    })
}

/// Resume points — for the episodes they were found for.
fn apply(episodes: &mut [Episode], played: &HashMap<String, ResumePoint>) {
    for episode in episodes {
        if let Some(point) = played.get(&episode.uri) {
            episode.resume_point = Some(point.clone());
        }
    }
}

fn failure_text(failure: &Failure) -> String {
    match failure {
        Failure::Definitive(error) => error.to_string(),
        Failure::Retry(error) => format!("{error:#}"),
    }
}

fn into_anyhow(failure: Failure) -> anyhow::Error {
    anyhow::anyhow!(failure_text(&failure))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A queryPodcastEpisodes answer (live, shortened): started, listened and new episodes.
    const PAGE: &str = r#"{"data":{"podcastUnionV2":{"__typename":"Podcast","episodesV2":{"__typename":"ContextEpisodePage",
        "items":[
          {"entity":{"_uri":"spotify:episode:a","data":{"__typename":"Episode","uri":"spotify:episode:a",
            "playedState":{"playPositionMilliseconds":61000,"state":"IN_PROGRESS"}}}},
          {"entity":{"_uri":"spotify:episode:b","data":{"__typename":"Episode",
            "playedState":{"playPositionMilliseconds":0,"state":"COMPLETED"}}}},
          {"entity":{"_uri":"spotify:episode:c","data":{"__typename":"Episode","uri":"spotify:episode:c",
            "playedState":{"playPositionMilliseconds":0,"state":"NOT_STARTED"}}}},
          {"entity":{"_uri":"spotify:episode:d","data":{"__typename":"RestrictedContent"}}}
        ],"pagingInfo":{"nextOffset":3},"totalCount":2762}}}}"#;

    #[test]
    fn played_states_read_as_web_api_resume_points() {
        let played = played_on_page(&serde_json::from_str(PAGE).unwrap());
        assert_eq!(played.len(), 3);
        assert_eq!(played["spotify:episode:a"], ResumePoint { fully_played: false, resume_position_ms: 61_000 });
        assert!(played["spotify:episode:b"].fully_played, "the URI comes from the wrapper when the episode lacks it");
        assert_eq!(played["spotify:episode:c"], ResumePoint::default());
        assert!(played_on_page(&json!({"errors": [{"message": "x"}]})).is_empty());
    }

    #[test]
    fn a_single_episode_reads_its_played_state() {
        let body = json!({"data": {"episodeUnionV2": {"__typename": "Episode",
            "playedState": {"playPositionMilliseconds": 1500, "state": "IN_PROGRESS"}}}});
        let point = played_of(body.pointer("/data/episodeUnionV2").unwrap()).unwrap();
        assert_eq!(point.resume_position_ms, 1500);
        assert!(played_of(&json!({"__typename": "Chapter", "playedState": {}})).is_none());
        assert!(played_of(&json!({"__typename": "Episode"})).is_none());
    }

    #[test]
    fn resume_points_go_only_to_episodes_that_have_one() {
        let mut episodes = vec![
            Episode { uri: "spotify:episode:a".into(), duration_ms: 120_000, ..Default::default() },
            Episode { uri: "spotify:episode:z".into(), ..Default::default() },
        ];
        apply(&mut episodes, &played_on_page(&serde_json::from_str(PAGE).unwrap()));
        assert_eq!(episodes[0].resume_ms(), Some(61_000));
        assert!(episodes[1].resume_point.is_none());
    }
}
