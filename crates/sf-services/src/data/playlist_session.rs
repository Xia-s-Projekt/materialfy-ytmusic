//! Playlists through a librespot session of the same account (sf_player::engine::session_playlists): the library
//! list from rootlist, edits, creation, following, visibility, cover — with no Web API quota.
//! None — there is no session or it could not do it: a repeatable write and a read can go to the Web API. A non-repeatable
//! write (adding, moving) whose answer was lost — is an error, not a retry through the Web API (it may have gone through).
use std::time::Duration;

use sf_api::models::{Image, Playlist};
use sf_api::{ApiError, ApiGateway};
use sf_player::PlayerHandle;
use sf_player::engine::session_playlists::{self as session, Change};
use sf_player::engine::session_reads::Failure;

pub use sf_player::engine::session_playlists::Library;

use super::route;

/// How long to wait for a write through the session.
const WRITE_TIMEOUT: Duration = Duration::from_secs(20);
/// How long to wait for the library list (rootlist and owner names).
const LIBRARY_TIMEOUT: Duration = Duration::from_secs(30);
/// How long to wait for the session to be ready at sign-in before asking the Web API for the list.
pub const SESSION_WAIT: Duration = Duration::from_secs(12);

/// A session answer as a Web API answer; None — ask the Web API (for a non-repeatable write — only if
/// the request never arrived).
fn settle<T>(result: Result<Result<T, Failure>, tokio::time::error::Elapsed>, repeatable: bool) -> Option<Result<T, ApiError>> {
    match result {
        Ok(Ok(value)) => Some(Ok(value)),
        Ok(Err(Failure::Definitive(error))) => Some(Err(error)),
        Ok(Err(Failure::Retry(error))) => {
            tracing::debug!("playlist via session failed: {error:#}; asking Web API");
            None
        }
        Err(_) if repeatable => {
            tracing::debug!("playlist via session timed out; asking Web API");
            None
        }
        Err(_) => Some(Err(ApiError::Network("Spotify did not answer in time — refresh the playlist".into()))),
    }
}

async fn run<T>(
    api: &ApiGateway,
    player: &PlayerHandle,
    repeatable: bool,
    run: impl AsyncFnOnce(sf_player::Session) -> Result<T, Failure>,
) -> Option<Result<T, ApiError>> {
    let session = route::account_session(api, player)?;
    settle(tokio::time::timeout(WRITE_TIMEOUT, run(session)).await, repeatable)
}

/// A write through the session. It bypasses the Web API, so after success we forget its GET cache (20 s):
/// otherwise the fallback read through the Web API would return the playlist from before the edit.
async fn write<T>(
    api: &ApiGateway,
    player: &PlayerHandle,
    repeatable: bool,
    edit: impl AsyncFnOnce(sf_player::Session) -> Result<T, Failure>,
) -> Option<Result<T, ApiError>> {
    let result = run(api, player, repeatable, edit).await;
    if matches!(result, Some(Ok(_))) {
        api.forget_cached();
    }
    result
}

/// A session of the same account; waits for it up to `wait` (sign-in: the engine is still connecting).
pub async fn wait_session(api: &ApiGateway, player: &PlayerHandle, wait: Duration) -> Option<sf_player::Session> {
    let deadline = tokio::time::Instant::now() + wait;
    loop {
        if let Some(session) = route::account_session(api, player) {
            return Some(session);
        }
        if tokio::time::Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// Library playlists: the rootlist tree and headers; None — the session is not ready or did not answer.
pub async fn library(api: &ApiGateway, player: &PlayerHandle, wait: Duration) -> Option<Library> {
    let session = wait_session(api, player, wait).await?;
    match tokio::time::timeout(LIBRARY_TIMEOUT, session::library(&session)).await {
        Ok(Ok(library)) => Some(library),
        Ok(Err(Failure::Definitive(error))) => {
            tracing::debug!("rootlist not loaded: {error}");
            None
        }
        Ok(Err(Failure::Retry(error))) => {
            tracing::debug!("rootlist not loaded: {error:#}");
            None
        }
        Err(_) => {
            tracing::debug!("rootlist load timed out");
            None
        }
    }
}

/// A cover collage for a playlist without its own; empty — it did not work out.
pub async fn mosaic(api: &ApiGateway, player: &PlayerHandle, id: &str) -> Vec<Image> {
    let Some(session) = route::account_session(api, player) else { return Vec::new() };
    tokio::time::timeout(WRITE_TIMEOUT, session::mosaic(&session, id)).await.unwrap_or_default()
}

/// A playlist edit; Ok — snapshot_id after the write.
pub async fn change(api: &ApiGateway, player: &PlayerHandle, id: &str, change: Change) -> Option<Result<Option<String>, ApiError>> {
    let repeatable = !matches!(change, Change::Add { .. } | Change::Move { .. });
    write(api, player, repeatable, async move |session| session::change(&session, id, &change).await).await
}

/// Which of `uris` are already in the playlist.
pub async fn duplicates(api: &ApiGateway, player: &PlayerHandle, id: &str, uris: &[String]) -> Option<Result<Vec<String>, ApiError>> {
    run(api, player, true, async move |session| session::duplicates(&session, id, uris).await).await
}

pub async fn create(api: &ApiGateway, player: &PlayerHandle, name: &str, public: bool) -> Option<Result<Playlist, ApiError>> {
    write(api, player, true, async move |session| session::create(&session, name, "", public).await).await
}

pub async fn follow(api: &ApiGateway, player: &PlayerHandle, id: &str) -> Option<Result<(), ApiError>> {
    let uri = format!("spotify:playlist:{id}");
    write(api, player, true, async move |session| session::follow(&session, &uri, None).await).await
}

pub async fn unfollow(api: &ApiGateway, player: &PlayerHandle, id: &str) -> Option<Result<(), ApiError>> {
    let uri = format!("spotify:playlist:{id}");
    write(api, player, true, async move |session| session::unfollow(&session, &uri).await).await
}

pub async fn set_public(api: &ApiGateway, player: &PlayerHandle, id: &str, public: bool) -> Option<Result<(), ApiError>> {
    let uri = format!("spotify:playlist:{id}");
    write(api, player, true, async move |session| session::set_public(&session, &uri, public).await).await
}

/// A cover from JPEG; Ok — snapshot_id after the write.
pub async fn upload_cover(api: &ApiGateway, player: &PlayerHandle, id: &str, jpeg: Vec<u8>) -> Option<Result<Option<String>, ApiError>> {
    let http = api.http().client().ok()?;
    write(api, player, true, async move |session| session::upload_cover(&session, &http, id, jpeg).await).await
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn elapsed() -> tokio::time::error::Elapsed {
        tokio::time::timeout(Duration::ZERO, std::future::pending::<()>()).await.unwrap_err()
    }

    #[tokio::test]
    async fn a_lost_answer_is_retried_only_for_repeatable_writes() {
        assert!(settle::<()>(Err(elapsed().await), true).is_none());
        assert!(matches!(settle::<()>(Err(elapsed().await), false), Some(Err(ApiError::Network(_)))));
        assert!(settle::<()>(Ok(Err(Failure::Retry(anyhow::anyhow!("x")))), false).is_none());
        let refused = Failure::Definitive(ApiError::Status { status: 403, message: String::new() });
        assert!(matches!(settle::<()>(Ok(Err(refused)), true), Some(Err(ApiError::Status { status: 403, .. }))));
        assert!(matches!(settle(Ok(Ok(5)), false), Some(Ok(5))));
    }
}
