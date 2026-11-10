//! Catalog (provider "services-core"): album, artist, search (the "best" heuristic), show and episodes.
//! No state except a 10-minute cache; routing the shared/personal application — ApiGateway::client_for(Operation).
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use md3_model::{
    AlbumInfo, AlbumItem, ArtistInfo, ArtistItem, EpisodeItem, PlaylistItem, SearchBest, SearchTotals, ShowItem, TrackDetails,
    TrackItem,
};
use sf_api::map;
use sf_api::models::{self, Album, Artist, Track};
use sf_api::{ApiError, Operation};
use sf_player::PlayerHandle;
use sf_player::engine::session_reads;
use tokio::sync::Semaphore;

use crate::auth::AuthService;
use crate::data::{album_overview, artist_overview, known, pathfinder, podcast_session, route};
use crate::{ServiceError, lock};

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
    /// "all" | "tracks" | "albums" | "artists" | "playlists" | "shows" | "episodes"; anything else → All
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

    /// Web API search types: catalog and playlists (only the shared app searches playlists).
    fn api_types(self) -> (Vec<&'static str>, bool) {
        match self {
            Self::All => (vec!["track", "artist", "album", "show", "episode"], true),
            Self::Tracks => (vec!["track"], false),
            Self::Albums => (vec!["album"], false),
            Self::Artists => (vec!["artist"], false),
            Self::Playlists => (Vec::new(), true),
            Self::Shows => (vec!["show"], false),
            Self::Episodes => (vec!["episode"], false),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AlbumPage {
    pub info: AlbumInfo,
    /// all tracks (long albums — paginated by 50)
    pub tracks: Vec<TrackItem>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArtistPage {
    pub info: ArtistInfo,
    /// /artists/{id}/top-tracks
    pub popular: Vec<TrackItem>,
    /// album,single
    pub albums: Vec<AlbumItem>,
    /// appears_on,compilation
    pub also_albums: Vec<AlbumItem>,
    /// related-artists (may be empty — closed for new applications)
    pub similar: Vec<ArtistItem>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchResults {
    pub tracks: Vec<TrackItem>,
    pub albums: Vec<AlbumItem>,
    pub artists: Vec<ArtistItem>,
    pub playlists: Vec<PlaylistItem>,
    pub shows: Vec<ShowItem>,
    pub episodes: Vec<EpisodeItem>,
    pub totals: SearchTotals,
    /// heuristic: the artist with a matching name, otherwise the first track
    pub best: Option<SearchBest>,
}

impl SearchResults {
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
            && self.albums.is_empty()
            && self.artists.is_empty()
            && self.playlists.is_empty()
            && self.shows.is_empty()
            && self.episodes.is_empty()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShowPage {
    pub info: ShowItem,
    /// the first page of episodes (newest first)
    pub episodes: Vec<EpisodeItem>,
    pub total: i64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EpisodePage {
    pub info: EpisodeItem,
    pub show: ShowItem,
    /// 2–3 neighboring episodes for "More episodes"
    pub more: Vec<EpisodeItem>,
}

/// Catalog page cache.
const FRESH: Duration = Duration::from_secs(10 * 60);
const CACHE_KEPT: usize = 200;
/// A longer album is cut (compilations of 1000 tracks).
const MAX_ALBUM_PAGES: u32 = 40;
/// Show episode page.
const EPISODE_PAGE: u32 = 50;

#[derive(Clone)]
enum Cached {
    Album(AlbumPage),
    Artist(ArtistPage),
    Show(ShowPage),
    Episode(EpisodePage),
    Track(TrackItem),
}

pub struct CatalogService {
    auth: Arc<AuthService>,
    player: PlayerHandle,
    cache: Mutex<HashMap<String, (Instant, Cached)>>,
    /// artist preview on hover — no more than two loads at once
    artists: Semaphore,
}

/// How much the page's optional requests (subscription, similar) may lag behind the main read:
/// under 429 the shared app answers for minutes, and the page is useful without them anyway.
const EXTRAS_GRACE: Duration = Duration::from_millis(1500);
/// How long the album page waits for the GraphQL gateway at all (the plays are optional).
const GATEWAY_DEADLINE: Duration = Duration::from_secs(2);
/// The "About the track" window: each request's deadline (session, gateway, album) — what did not make it is not shown.
const DETAILS_DEADLINE: Duration = Duration::from_secs(5);

/// The `main` answer; the `extras` answer — if it made it together with it or no later than `grace` after.
async fn with_extras<M: Future, E: Future>(main: M, extras: E, grace: Duration) -> (M::Output, Option<E::Output>) {
    tokio::pin!(main, extras);
    tokio::select! {
        biased;
        out = &mut main => (out, tokio::time::timeout(grace, extras).await.ok()),
        found = &mut extras => (main.await, Some(found)),
    }
}

/// Only the sections of search kind `kind` (the gateway answers with all at once).
fn only(kind: SearchKind, mut results: models::SearchResults) -> models::SearchResults {
    let (types, playlists) = kind.api_types();
    let wanted = |name: &str| types.contains(&name);
    if !wanted("track") {
        results.tracks = None;
    }
    if !wanted("artist") {
        results.artists = None;
    }
    if !wanted("album") {
        results.albums = None;
    }
    if !wanted("show") {
        results.shows = None;
    }
    if !wanted("episode") {
        results.episodes = None;
    }
    if !playlists {
        results.playlists = None;
    }
    results
}

/// A page section that did not load stays empty instead of dropping the page.
fn or_empty<T>(section: Result<Vec<T>, ApiError>, name: &str, id: &str) -> Vec<T> {
    section.unwrap_or_else(|error| {
        tracing::debug!("{name} ({id}): {error:#}");
        Vec::new()
    })
}

/// Translating a Web API error into a service error; NotSignedIn — separately.
fn service_error(error: ApiError) -> ServiceError {
    match error {
        ApiError::NotSignedIn => ServiceError::NotSignedIn,
        other => ServiceError::Api(other),
    }
}

/// Album tracks come without the nested album — return it for the covers and links.
fn with_album(mut tracks: Vec<Track>, album: &Album) -> Vec<Track> {
    let mut bare = album.clone();
    bare.tracks = None;
    for track in &mut tracks {
        if track.album.is_none() {
            track.album = Some(bare.clone());
        }
    }
    tracks
}

/// Assemble the album page from the Web API answers.
pub fn album_page(album: &Album, tracks: Vec<Track>) -> AlbumPage {
    let tracks = with_album(tracks, album);
    known::remember_recordings(tracks.iter());
    let items: Vec<TrackItem> = tracks.iter().map(map::track_item).collect();
    known::remember(items.iter());
    AlbumPage { info: map::album_info(album, &tracks), tracks: items }
}

/// Artist page: similar and popular may be absent.
pub fn artist_page(
    artist: &Artist,
    followed: bool,
    popular: &[Track],
    albums: &[Album],
    also: &[Album],
    similar: &[Artist],
) -> ArtistPage {
    known::remember_recordings(popular.iter());
    let popular: Vec<TrackItem> = popular.iter().map(map::track_item).collect();
    known::remember(popular.iter());
    ArtistPage {
        info: map::artist_info(artist, followed),
        popular,
        albums: latest_release_first(albums),
        also_albums: also.iter().map(map::album_item).collect(),
        similar: similar.iter().map(map::artist_item).collect(),
    }
}

/// First — the last release, the rest in the order Spotify gave them: the "Albums and singles"
/// section labels the first tile, and it must be exactly the fresh one. There are no dates — the order is not touched.
fn latest_release_first(albums: &[Album]) -> Vec<AlbumItem> {
    // the full date is only here: AlbumItem keeps one year, and releases of one year cannot be told apart by it
    let newest = albums
        .iter()
        .enumerate()
        .filter(|(_, album)| album.release_date.as_deref().is_some_and(|date| !date.is_empty()))
        .max_by(|(left_index, left), (right_index, right)| {
            left.release_date.cmp(&right.release_date).then(right_index.cmp(left_index))
        })
        .map(|(index, _)| index);
    let mut mapped: Vec<AlbumItem> = albums.iter().map(map::album_item).collect();
    if let Some(newest) = newest {
        let latest = mapped.remove(newest);
        mapped.insert(0, latest);
    }
    mapped
}

/// The "About the track" window from what managed to arrive: session metadata, the getTrack answer, the album page.
pub fn track_details(
    track_id: &str,
    track: Option<&Track>,
    overview: Option<&album_overview::TrackOverview>,
    album: Option<&AlbumPage>,
) -> TrackDetails {
    let mut details = TrackDetails { popularity: -1, ..Default::default() };
    if let Some(track) = track {
        details.track = map::track_item(track);
        details.track_number = track.track_number.map_or(0, i64::from);
        details.disc_number = track.disc_number.map_or(0, i64::from);
        details.isrc = track.external_ids.isrc.clone().unwrap_or_default();
        details.popularity = track.popularity.map_or(-1, i64::from);
        if let Some(album) = &track.album {
            details.release_date = album.release_date.clone().unwrap_or_default();
            details.label = album.label.clone().unwrap_or_default();
        }
    } else if let Some(row) = album.and_then(|page| page.tracks.iter().find(|row| row.track_id == track_id)) {
        details.track = row.clone();
    }
    if details.track.track_id.is_empty() {
        details.track.track_id = track_id.to_owned();
        details.track.uri = format!("spotify:track:{track_id}");
    }
    if let Some(page) = album {
        let info = &page.info;
        details.track.play_count = album_overview::plays_on(page, track_id);
        details.album_kind = info.item.kind.clone();
        details.album_track_count = info.item.track_count;
        if !info.label.is_empty() {
            details.label = info.label.clone();
        }
        if info.release_date.len() > details.release_date.len() {
            details.release_date = info.release_date.clone();
        }
        details.copyrights = info.copyrights.clone();
        details.courtesy_line = info.courtesy_line.clone();
        if details.track.album.is_empty() {
            details.track.album = info.item.title.clone();
            details.track.album_id = info.item.album_id.clone();
        }
        if details.track.cover.is_empty() {
            details.track.cover = info.item.cover.clone();
            details.track.cover_large = info.item.cover_large.clone();
        }
    }
    if let Some(overview) = overview {
        if overview.play_count > 0 {
            details.track.play_count = overview.play_count;
        }
        if details.track_number == 0 {
            details.track_number = overview.track_number;
        }
        if overview.release_date.len() > details.release_date.len() {
            details.release_date = overview.release_date.clone();
        }
        if details.copyrights.is_empty() {
            details.copyrights = overview.copyrights.clone();
        }
        if details.courtesy_line.is_empty() {
            details.courtesy_line = overview.courtesy_line.clone();
        }
    }
    details
}

/// The best result: the artist whose name matched the query (or there are no tracks), otherwise the first track, album,
/// playlist, show (ui/search.rs::all in spotifast).
pub fn best_result(query: &str, results: &models::SearchResults) -> Option<SearchBest> {
    fn best<T: serde::Serialize>(kind: &str, item: T) -> Option<SearchBest> {
        match serde_json::to_value(item) {
            Ok(serde_json::Value::Object(item)) => Some(SearchBest { kind: kind.to_owned(), item }),
            _ => None,
        }
    }
    fn first<T>(page: &Option<models::Page<T>>) -> Option<&T> {
        page.as_ref().and_then(|page| page.items.first())
    }
    let query = query.trim().to_lowercase();
    let tracks = results.tracks.as_ref().map(|page| page.items.as_slice()).unwrap_or_default();
    if let Some(artist) = first(&results.artists).filter(|artist| artist.name.to_lowercase() == query || tracks.is_empty())
    {
        return best("artist", map::artist_item(artist));
    }
    if let Some(track) = tracks.first() {
        return best("track", map::track_item(track));
    }
    if let Some(album) = first(&results.albums) {
        return best("album", map::album_item(album));
    }
    if let Some(playlist) = first(&results.playlists) {
        return best("playlist", map::playlist_item(playlist, None));
    }
    first(&results.shows).and_then(|show| best("show", map::show_item(show)))
}

/// Web API search answer → results for QML.
pub fn search_results(query: &str, results: &models::SearchResults, me: Option<&md3_model::AccountId>) -> SearchResults {
    let total = |page: Option<u32>| page.map(i64::from).unwrap_or(0);
    if let Some(page) = &results.tracks {
        known::remember_recordings(page.items.iter());
    }
    let tracks: Vec<TrackItem> =
        results.tracks.as_ref().map(|page| page.items.iter().map(map::track_item).collect()).unwrap_or_default();
    known::remember(tracks.iter());
    SearchResults {
        tracks,
        albums: results.albums.as_ref().map(|p| p.items.iter().map(map::album_item).collect()).unwrap_or_default(),
        artists: results.artists.as_ref().map(|p| p.items.iter().map(map::artist_item).collect()).unwrap_or_default(),
        playlists: results
            .playlists
            .as_ref()
            .map(|p| p.items.iter().map(|pl| map::playlist_item(pl, me)).collect())
            .unwrap_or_default(),
        shows: results.shows.as_ref().map(|p| p.items.iter().map(map::show_item).collect()).unwrap_or_default(),
        episodes: results
            .episodes
            .as_ref()
            .map(|p| p.items.iter().map(|ep| map::episode_item(ep, None)).collect())
            .unwrap_or_default(),
        totals: SearchTotals {
            tracks: total(results.tracks.as_ref().map(|p| p.total)),
            albums: total(results.albums.as_ref().map(|p| p.total)),
            artists: total(results.artists.as_ref().map(|p| p.total)),
            playlists: total(results.playlists.as_ref().map(|p| p.total)),
            shows: total(results.shows.as_ref().map(|p| p.total)),
            episodes: total(results.episodes.as_ref().map(|p| p.total)),
        },
        best: best_result(query, results),
    }
}

impl CatalogService {
    pub fn new(auth: Arc<AuthService>, player: PlayerHandle) -> Arc<Self> {
        Arc::new(Self { auth, player, cache: Mutex::new(HashMap::new()), artists: Semaphore::new(2) })
    }

    fn cached(&self, key: &str) -> Option<Cached> {
        let cache = lock(&self.cache);
        cache.get(key).filter(|(at, _)| at.elapsed() < FRESH).map(|(_, value)| value.clone())
    }

    fn store(&self, key: String, value: Cached) {
        let mut cache = lock(&self.cache);
        if cache.len() >= CACHE_KEPT {
            cache.retain(|_, (at, _)| at.elapsed() < FRESH);
            if cache.len() >= CACHE_KEPT
                && let Some(oldest) = cache.iter().min_by_key(|(_, (at, _))| *at).map(|(key, _)| key.clone())
            {
                cache.remove(&oldest);
            }
        }
        cache.insert(key, (Instant::now(), value));
    }

    /// Forget the cache (account change, saving changed "saved").
    pub fn forget(&self) {
        lock(&self.cache).clear();
    }

    async fn client(&self, operation: Operation) -> Result<Arc<sf_api::ApiClient>, ServiceError> {
        self.auth.api().client_for(operation).await.map_err(|error| self.fail(error))
    }

    fn fail(&self, error: ApiError) -> ServiceError {
        self.auth.note_api_error(&error);
        service_error(error)
    }

    pub async fn album(&self, album_id: &str) -> Result<AlbumPage, ServiceError> {
        let key = format!("album:{album_id}");
        if let Some(Cached::Album(page)) = self.cached(&key) {
            return Ok(page);
        }
        // The album and all its tracks — from the librespot session (without the Web API quota); Web API — a fallback path.
        // In parallel — the GraphQL gateway (getAlbum: track plays, label, copyright holders), which does not
        // hold the page longer than GATEWAY_DEADLINE and EXTRAS_GRACE after the session read.
        let api = self.auth.api();
        let http = api.http();
        let session = route::account_session(&api, &self.player);
        let read = async {
            match &session {
                Some(session) => route::session_read(session_reads::album_with_tracks(session, album_id)).await,
                None => None,
            }
        };
        let overview = async {
            let asked = album_overview::album(&http, session.as_ref()?, album_id);
            match tokio::time::timeout(GATEWAY_DEADLINE, asked).await {
                Ok(Ok(overview)) => Some(overview),
                Ok(Err(error)) => {
                    tracing::debug!("album {album_id} via gateway: {error:#}");
                    None
                }
                Err(_) => {
                    tracing::debug!("album {album_id} via gateway: no answer in {GATEWAY_DEADLINE:?}");
                    None
                }
            }
        };
        let (read, overview) = with_extras(read, overview, EXTRAS_GRACE).await;
        let overview = overview.flatten();
        let (album, tracks, complete) = match read {
            Some(Ok((album, tracks))) => (album, tracks, true),
            Some(Err(error)) => return Err(self.fail(error)),
            None => self.api_album(album_id).await?,
        };
        let mut page = album_page(&album, tracks);
        if let Some(overview) = &overview {
            album_overview::apply(&mut page, overview);
        }
        // a truncated list is not cached: the next opening will read it to the end; without the gateway's answer — too
        // (the plays will appear on the next opening)
        if complete && (overview.is_some() || session.is_none()) {
            self.store(key, Cached::Album(page.clone()));
        }
        Ok(page)
    }

    /// The album through Web API. Only the album itself with the first page of tracks is required: the next page,
    /// which did not load (429, network), cuts the list, not the page; the third field — the list is full.
    async fn api_album(&self, album_id: &str) -> Result<(Album, Vec<Track>, bool), ServiceError> {
        let client = self.client(Operation::Catalog).await?;
        let album = client.album(album_id).await.map_err(|e| self.fail(e))?;
        let first = album.tracks.clone().unwrap_or_default();
        let mut next = first.next_offset();
        let mut tracks = first.items;
        for _ in 0..MAX_ALBUM_PAGES {
            let Some(offset) = next else { break };
            match client.album_tracks(album_id, offset, 50).await {
                Ok(page) => {
                    next = page.next_offset().filter(|n| *n > offset);
                    tracks.extend(page.items);
                }
                Err(error) => {
                    tracing::debug!("album {album_id} tracks from {offset}: {error:#}");
                    return Ok((album, tracks, false));
                }
            }
        }
        Ok((album, tracks, true))
    }

    pub async fn artist(&self, artist_id: &str) -> Result<ArtistPage, ServiceError> {
        let key = format!("artist:{artist_id}");
        if let Some(Cached::Artist(page)) = self.cached(&key) {
            return Ok(page);
        }
        let _permit = self.artists.acquire().await.map_err(|_| ServiceError::Other(mf_i18n::t("The catalog has stopped")))?;
        if let Some(Cached::Artist(page)) = self.cached(&key) {
            return Ok(page);
        }
        // The page — without the Web API quota (the shared app answers 429 for a long time, and the artist needs five
        // requests): librespot session metadata (artist, popular, releases) and in parallel the GraphQL gateway
        // (queryArtistOverview: followers, listeners, subscription, similar, biography, header), which does not hold
        // the page longer than EXTRAS_GRACE. Web API — only a fallback path: the whole page if the session could not;
        // the subscription and similar if the gateway did not answer (also no longer than EXTRAS_GRACE).
        let api = self.auth.api();
        let session = route::account_session(&api, &self.player);
        let read = async {
            match &session {
                Some(session) => route::session_read(session_reads::artist(session, artist_id)).await,
                None => None,
            }
        };
        let overview = async {
            let result = artist_overview::artist(&api.http(), session.as_ref()?, artist_id).await;
            result.map_err(|error| tracing::debug!("artist {artist_id} via gateway: {error:#}")).ok()
        };
        let (read, overview) = with_extras(read, overview, EXTRAS_GRACE).await;
        let overview = overview.flatten();
        let uri = format!("spotify:artist:{artist_id}");
        // the session already gave the similar ones — Web API does not go for them
        let session_related = matches!(&read, Some(Ok(record)) if !record.related.is_empty());
        let main = async {
            match read {
                Some(Ok(record)) => Ok((record.artist, record.top_tracks, record.albums, record.appears_on, record.related)),
                Some(Err(error)) => Err(self.fail(error)),
                None => self.api_artist(artist_id).await.map(|(artist, popular, albums, also)| (artist, popular, albums, also, Vec::new())),
            }
        };
        let extras = async {
            match overview {
                Some(_) => (false, Vec::new()),
                None => tokio::join!(self.followed(&uri), async {
                    if session_related { Vec::new() } else { self.related(artist_id).await }
                }),
            }
        };
        let (main, extras) = with_extras(main, extras, EXTRAS_GRACE).await;
        let (artist, popular, albums, also, similar) = main?;
        let (followed, related) = extras.unwrap_or_default();
        let similar = if similar.is_empty() { related } else { similar };
        let mut page = artist_page(&artist, followed, &popular, &albums, &also, &similar);
        if let Some(overview) = &overview {
            artist_overview::apply(&mut page, overview);
        }
        self.store(key, Cached::Artist(page.clone()));
        Ok(page)
    }

    /// The artist through Web API. Only the artist itself is required: a section that did not load
    /// (popular is closed for apps in development mode, 429, limits) stays empty.
    async fn api_artist(&self, artist_id: &str) -> Result<(Artist, Vec<Track>, Vec<Album>, Vec<Album>), ServiceError> {
        let catalog = self.client(Operation::Catalog).await?;
        let artist = catalog.artist(artist_id).await.map_err(|e| self.fail(e))?;
        let popular = async {
            let restricted = self.auth.api().client_for(Operation::UnsupportedDevelopmentMode).await?;
            restricted.artist_top_tracks(artist_id).await
        };
        let (popular, albums, also) = tokio::join!(
            popular,
            catalog.artist_albums(artist_id, "album,single", 0, 50),
            catalog.artist_albums(artist_id, "appears_on,compilation", 0, 50),
        );
        let popular = or_empty(popular, "popular", artist_id);
        let albums = or_empty(albums.map(|page| page.items), "albums", artist_id);
        let also = or_empty(also.map(|page| page.items), "appears on", artist_id);
        Ok((artist, popular, albums, also))
    }

    /// Similar artists (closed for new applications — then empty).
    async fn related(&self, artist_id: &str) -> Vec<Artist> {
        let Ok(client) = self.auth.api().client_for(Operation::UnsupportedDevelopmentMode).await else {
            return Vec::new();
        };
        client.related_artists(artist_id).await.unwrap_or_else(|error| {
            tracing::debug!("related artists {artist_id}: {error:#}");
            Vec::new()
        })
    }

    /// Artist subscription (an error — "not subscribed").
    async fn followed(&self, uri: &str) -> bool {
        if uri.is_empty() {
            return false;
        }
        // the session's collection, then Web API with a short deadline
        crate::data::collection_session::contains(&self.auth.api(), &self.player, &[uri.to_owned()])
            .await
            .ok()
            .and_then(|found| found.first().copied())
            .unwrap_or(false)
    }

    pub async fn search(&self, query: &str, kind: SearchKind) -> Result<SearchResults, ServiceError> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(SearchResults::default());
        }
        let api = self.auth.api();
        // First — the Spotify clients' gateway through the session (all sections in one request, without the Web API quota)
        if let Some(session) = route::account_session(&api, &self.player) {
            match pathfinder::search(&api.http(), &session, query).await {
                Ok(results) => return Ok(self.searched(query, kind, only(kind, results))),
                Err(error) => tracing::debug!("search via session failed: {error:#}; asking Web API"),
            }
        }
        let (catalog_types, playlists) = kind.api_types();
        let split = api.personal_ready();
        let mut types = catalog_types.clone();
        if playlists && !split {
            types.push("playlist");
        }
        let catalog = async {
            if types.is_empty() {
                return Ok(models::SearchResults::default());
            }
            let operation = if split { Operation::CatalogSearch } else { Operation::PlaylistSearch };
            api.client_for(operation).await?.search(query, &types).await
        };
        let playlist_half = async {
            if !(playlists && split) {
                return Ok(None);
            }
            let client = api.client_for(Operation::PlaylistSearch).await?;
            client.search(query, &["playlist"]).await.map(|results| results.playlists)
        };
        let (catalog, playlist_half) = tokio::join!(catalog, playlist_half);
        let results = match (catalog, playlist_half) {
            (Ok(mut results), Ok(Some(playlists))) => {
                results.playlists = Some(playlists);
                results
            }
            (Ok(results), Ok(None)) => results,
            (Ok(results), Err(error)) => {
                tracing::debug!("playlist search failed: {error:#}");
                results
            }
            (Err(error), Ok(Some(playlists))) => {
                tracing::debug!("catalog search failed: {error:#}");
                models::SearchResults { playlists: Some(playlists), ..Default::default() }
            }
            (Err(error), _) => return Err(self.fail(error)),
        };
        Ok(self.searched(query, kind, results))
    }

    /// Search answer → results for QML; the "best" — only in "All".
    fn searched(&self, query: &str, kind: SearchKind, results: models::SearchResults) -> SearchResults {
        let api = self.auth.api();
        if let Some(page) = &results.playlists {
            api.observe_playlists(page.items.iter());
        }
        let me = api.account();
        let mut mapped = search_results(query, &results, me.as_ref());
        if kind != SearchKind::All {
            mapped.best = None;
        }
        mapped
    }

    pub async fn show(&self, show_id: &str) -> Result<ShowPage, ServiceError> {
        let key = format!("show:{show_id}");
        if let Some(Cached::Show(page)) = self.cached(&key) {
            return Ok(page);
        }
        // The show and the first page of episodes — from the librespot session (without the Web API quota); Web API — a fallback path.
        let (show, first) = match self.session_show(show_id, 0).await {
            Some(Ok(read)) => read,
            Some(Err(error)) => return Err(self.fail(error)),
            None => {
                let client = self.client(Operation::Catalog).await?;
                let show = client.show(show_id).await.map_err(|e| self.fail(e))?;
                let first = match show.episodes.clone() {
                    Some(page) if !page.items.is_empty() => page,
                    _ => client.show_episodes(show_id, 0, EPISODE_PAGE).await.map_err(|e| self.fail(e))?,
                };
                (show, first)
            }
        };
        let total = i64::from(show.total_episodes.unwrap_or(first.total));
        let page = ShowPage {
            info: map::show_item(&show),
            episodes: first.items.iter().map(|ep| map::episode_item(ep, Some(&show))).collect(),
            total,
        };
        self.store(key, Cached::Show(page.clone()));
        Ok(page)
    }

    /// The next page of episodes (by 50).
    pub async fn show_episodes(&self, show_id: &str, offset: u32) -> Result<Vec<EpisodeItem>, ServiceError> {
        let page = match self.session_show(show_id, offset).await {
            Some(Ok((_, page))) => page,
            Some(Err(error)) => return Err(self.fail(error)),
            None => {
                let client = self.client(Operation::Catalog).await?;
                client.show_episodes(show_id, offset, EPISODE_PAGE).await.map_err(|e| self.fail(e))?
            }
        };
        let show = match self.cached(&format!("show:{show_id}")) {
            Some(Cached::Show(page)) => Some(page.info),
            _ => None,
        };
        Ok(page
            .items
            .iter()
            .map(|ep| {
                let mut item = map::episode_item(ep, None);
                if let Some(show) = &show {
                    if item.show_id.is_empty() {
                        item.show_id = show.show_id.clone();
                    }
                    if item.show_name.is_empty() {
                        item.show_name = show.name.clone();
                    }
                    if item.cover.is_empty() {
                        item.cover = show.cover.clone();
                        item.cover_large = show.cover_large.clone();
                    }
                }
                item
            })
            .collect())
    }

    pub async fn episode(&self, episode_id: &str) -> Result<EpisodePage, ServiceError> {
        let key = format!("episode:{episode_id}");
        if let Some(Cached::Episode(page)) = self.cached(&key) {
            return Ok(page);
        }
        // The episode, its show and neighbors — from the librespot session; Web API — a fallback path.
        let (episode, show, more) = match self.session_episode(episode_id).await {
            Some(Ok(read)) => read,
            Some(Err(error)) => return Err(self.fail(error)),
            None => self.api_episode(episode_id).await?,
        };
        let more = more.iter().map(|other| map::episode_item(other, Some(&show))).collect();
        let page = EpisodePage { info: map::episode_item(&episode, Some(&show)), show: map::show_item(&show), more };
        known::remember([&page.info.to_track()]);
        self.store(key, Cached::Episode(page.clone()));
        Ok(page)
    }

    /// The show's header and the episode page with `offset` through the session; None — there is no session or it could not.
    async fn session_show(
        &self,
        show_id: &str,
        offset: u32,
    ) -> Option<Result<(models::Show, models::Page<models::Episode>), ApiError>> {
        let api = self.auth.api();
        let session = route::account_session(&api, &self.player)?;
        route::session_read(podcast_session::show_page(&api.http(), &session, show_id, offset, EPISODE_PAGE)).await
    }

    /// The episode, its show and neighboring episodes through the session; None — there is no session or it could not.
    async fn session_episode(
        &self,
        episode_id: &str,
    ) -> Option<Result<(models::Episode, models::Show, Vec<models::Episode>), ApiError>> {
        let api = self.auth.api();
        let session = route::account_session(&api, &self.player)?;
        route::session_read(podcast_session::episode_page(&api.http(), &session, episode_id)).await
    }

    /// The episode through Web API; neighboring episodes that did not load — empty.
    async fn api_episode(
        &self,
        episode_id: &str,
    ) -> Result<(models::Episode, models::Show, Vec<models::Episode>), ServiceError> {
        let client = self.client(Operation::Catalog).await?;
        let episode = client.episode(episode_id).await.map_err(|e| self.fail(e))?;
        let show = episode.show.clone().unwrap_or_default();
        let more = if show.id.is_empty() {
            Vec::new()
        } else {
            match client.show_episodes(&show.id, 0, 6).await {
                Ok(page) => page.items.into_iter().filter(|other| other.id != episode.id).take(3).collect(),
                Err(error) => {
                    tracing::debug!("neighbouring episodes: {error:#}");
                    Vec::new()
                }
            }
        };
        Ok((episode, show, more))
    }

    pub async fn track(&self, track_id: &str) -> Result<TrackItem, ServiceError> {
        let key = format!("track:{track_id}");
        if let Some(Cached::Track(item)) = self.cached(&key) {
            return Ok(item);
        }
        // The track — from the librespot session metadata; Web API — a fallback path.
        let session = route::account_session(&self.auth.api(), &self.player);
        let read = match &session {
            Some(session) => route::session_read(session_reads::track_by_id(session, track_id)).await,
            None => None,
        };
        let track = match read {
            Some(Ok(track)) => track,
            Some(Err(error)) => return Err(self.fail(error)),
            None => self.client(Operation::Catalog).await?.track(track_id).await.map_err(|e| self.fail(e))?,
        };
        known::remember_recordings([&track]);
        let item = map::track_item(&track);
        known::remember([&item]);
        self.store(key, Cached::Track(item.clone()));
        Ok(item)
    }

    /// Everything known about a track (the "About the track" window): session metadata (number, disc, ISRC, popularity),
    /// the GraphQL gateway (getTrack: plays, date, copyright holders) and the album page (label; from the cache if the
    /// album was opened recently) — at once, each no longer than DETAILS_DEADLINE. `album_id` — a hint from the track
    /// row (without it the album is learned from the metadata). An error — only if nobody answered.
    pub async fn track_details(&self, track_id: &str, album_id: &str) -> Result<TrackDetails, ServiceError> {
        let api = self.auth.api();
        let http = api.http();
        let session = route::account_session(&api, &self.player);
        let read = async {
            let session = session.as_ref()?;
            let read = route::session_read(session_reads::track_by_id(session, track_id));
            match tokio::time::timeout(DETAILS_DEADLINE, read).await {
                Ok(Some(Ok(track))) => Some(track),
                Ok(Some(Err(error))) => {
                    tracing::debug!("track {track_id}: {error:#}");
                    None
                }
                _ => None,
            }
        };
        let gateway = async {
            let asked = album_overview::track(&http, session.as_ref()?, track_id);
            match tokio::time::timeout(DETAILS_DEADLINE, asked).await {
                Ok(Ok(overview)) => Some(overview),
                Ok(Err(error)) => {
                    tracing::debug!("track {track_id} via gateway: {error:#}");
                    None
                }
                Err(_) => None,
            }
        };
        let album = |album_id: String| async move {
            if album_id.is_empty() {
                return None;
            }
            tokio::time::timeout(DETAILS_DEADLINE, self.album(&album_id)).await.ok()?.ok()
        };
        let (track, overview, mut page) = tokio::join!(read, gateway, album(album_id.to_owned()));
        if page.is_none()
            && album_id.is_empty()
            && let Some(id) = track.as_ref().and_then(|track| track.album.as_ref()).map(|album| album.id.clone())
        {
            page = album(id).await;
        }
        if track.is_none() && overview.is_none() && page.is_none() {
            return Err(ServiceError::Other(mf_i18n::t("Couldn't load")));
        }
        Ok(track_details(track_id, track.as_ref(), overview.as_ref(), page.as_ref()))
    }

    /// The player the catalog was created with.
    pub fn player(&self) -> &PlayerHandle {
        &self.player
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sf_api::models::Page;

    fn artist(name: &str) -> Artist {
        Artist { id: name.to_lowercase(), name: name.to_owned(), uri: format!("spotify:artist:{name}"), ..Default::default() }
    }

    fn page<T>(items: Vec<T>) -> Option<Page<T>> {
        let total = items.len() as u32;
        Some(Page { items, total, limit: 20, offset: 0, next: None })
    }

    #[test]
    fn search_kinds_split_catalog_and_playlists() {
        assert_eq!(SearchKind::from_name("shows"), SearchKind::Shows);
        assert_eq!(SearchKind::from_name("anything"), SearchKind::All);
        assert_eq!(SearchKind::Playlists.api_types(), (Vec::new(), true));
        assert_eq!(SearchKind::Tracks.api_types(), (vec!["track"], false));
    }

    #[test]
    fn a_search_through_the_session_keeps_only_the_asked_shelves() {
        let all = models::SearchResults {
            tracks: page(vec![track("t1")]),
            artists: page(vec![artist("Muse")]),
            albums: page(vec![Album::default()]),
            playlists: page(vec![models::Playlist::default()]),
            shows: page(vec![models::Show::default()]),
            episodes: page(vec![models::Episode::default()]),
        };
        assert_eq!(only(SearchKind::All, all.clone()), all);
        let tracks = only(SearchKind::Tracks, all.clone());
        assert!(tracks.tracks.is_some());
        assert!(tracks.artists.is_none() && tracks.albums.is_none() && tracks.playlists.is_none());
        let playlists = only(SearchKind::Playlists, all);
        assert!(playlists.playlists.is_some() && playlists.tracks.is_none() && playlists.shows.is_none());
    }

    #[test]
    fn nothing_found_has_no_best_result() {
        assert!(best_result("x", &models::SearchResults::default()).is_none());
    }

    fn track(id: &str) -> Track {
        Track { id: Some(id.to_owned()), name: format!("Song {id}"), uri: format!("spotify:track:{id}"), ..Default::default() }
    }

    #[test]
    fn an_artist_named_like_the_query_beats_the_first_song() {
        let results = models::SearchResults {
            artists: page(vec![artist("Muse")]),
            tracks: page(vec![track("t1")]),
            ..Default::default()
        };
        assert_eq!(best_result("  muse ", &results).unwrap().kind, "artist");
        let best = best_result("uprising", &results).unwrap();
        assert_eq!(best.kind, "track");
        assert_eq!(best.item["trackId"], "t1");
    }

    #[test]
    fn an_artist_without_songs_is_the_best_result() {
        let results = models::SearchResults { artists: page(vec![artist("Muse")]), ..Default::default() };
        let best = best_result("whatever", &results).unwrap();
        assert_eq!(best.kind, "artist");
        assert_eq!(best.item["name"], "Muse");
    }

    #[test]
    fn albums_then_playlists_then_shows_fill_in() {
        let album = Album { id: "a1".into(), name: "Album".into(), uri: "spotify:album:a1".into(), ..Default::default() };
        let results = models::SearchResults { albums: page(vec![album]), ..Default::default() };
        assert_eq!(best_result("x", &results).unwrap().kind, "album");
        let show = models::Show { id: "s1".into(), name: "Show".into(), uri: "spotify:show:s1".into(), ..Default::default() };
        let results = models::SearchResults { shows: page(vec![show]), ..Default::default() };
        assert_eq!(best_result("x", &results).unwrap().kind, "show");
    }

    #[test]
    fn album_tracks_get_their_album_back_for_covers_and_totals() {
        let album = Album {
            id: "a1".into(),
            name: "Album".into(),
            uri: "spotify:album:a1".into(),
            images: vec![models::Image { url: "big".into(), width: Some(640), height: Some(640) }],
            ..Default::default()
        };
        let mut first = track("t1");
        first.duration_ms = 1000;
        let mut second = track("t2");
        second.duration_ms = 2000;
        let page = album_page(&album, vec![first, second]);
        assert_eq!(page.tracks.len(), 2);
        assert!(page.tracks.iter().all(|row| row.album_id == "a1" && row.album == "Album"));
        assert_eq!(page.info.duration_ms, 3000);
        assert_eq!(page.info.item.album_id, "a1");
    }

    #[test]
    fn search_results_count_and_map_every_shelf() {
        let results = models::SearchResults {
            tracks: page(vec![track("t1"), track("t2")]),
            artists: page(vec![artist("Muse")]),
            ..Default::default()
        };
        let mapped = search_results("muse", &results, None);
        assert_eq!(mapped.totals.tracks, 2);
        assert_eq!(mapped.totals.artists, 1);
        assert_eq!(mapped.tracks[1].track_id, "t2");
        assert_eq!(mapped.best.as_ref().unwrap().kind, "artist");
        assert!(!mapped.is_empty());
        assert_eq!(known::get("spotify:track:t2").unwrap().title, "Song t2");
    }

    #[test]
    fn an_artist_page_keeps_missing_shelves_empty() {
        let page = artist_page(&artist("Muse"), true, &[track("t1")], &[], &[], &[]);
        assert!(page.info.followed);
        assert_eq!(page.popular.len(), 1);
        assert!(page.similar.is_empty());
    }

    #[test]
    fn the_artist_page_leads_with_the_latest_release_and_keeps_the_rest_in_order() {
        let release = |id: &str, date: &str| Album {
            id: id.into(),
            name: format!("Release {id}"),
            release_date: Some(date.into()),
            ..Default::default()
        };
        // Spotify gives albums, then singles: a fresh single stands at the end of the list
        let albums = [
            release("old-album", "2017-06-23"),
            release("album", "2022-11-05"),
            release("undated", ""),
            release("single", "2026-01-01"),
        ];
        let page = artist_page(&artist("Muse"), false, &[], &albums, &[], &[]);
        let ids: Vec<&str> = page.albums.iter().map(|album| album.album_id.as_str()).collect();
        assert_eq!(
            ids,
            ["single", "old-album", "album", "undated"],
            "the latest release leads, the rest keep Spotify's order"
        );
        assert_eq!(page.albums[0].year, 2026, "the caption in QML keys off the year");
    }

    #[test]
    fn an_artist_without_dated_releases_keeps_its_order() {
        let album = |id: &str| Album { id: id.into(), name: id.into(), ..Default::default() };
        let page = artist_page(&artist("Muse"), false, &[], &[album("a"), album("b")], &[], &[]);
        let ids: Vec<&str> = page.albums.iter().map(|album| album.album_id.as_str()).collect();
        assert_eq!(ids, ["a", "b"], "nothing to lead with, nothing moved");
    }

    #[test]
    fn a_failed_section_leaves_the_artist_page_standing() {
        // 403 for popular (development mode), 429 for albums: the page is assembled anyway
        let refused = ApiError::Status { status: 403, message: "Forbidden".into() };
        let popular: Vec<Track> = or_empty(Err(refused), "popular", "muse");
        let albums: Vec<Album> = or_empty(Err(ApiError::RateLimited), "albums", "muse");
        let also = or_empty(Ok(vec![Album { id: "a1".into(), name: "Live".into(), ..Default::default() }]), "", "muse");
        let page = artist_page(&artist("Muse"), false, &popular, &albums, &also, &[]);
        assert_eq!(page.info.item.name, "Muse");
        assert!(page.popular.is_empty() && page.albums.is_empty());
        assert_eq!(page.also_albums.len(), 1);
    }

    #[tokio::test]
    async fn slow_extras_do_not_hold_the_page() {
        let grace = Duration::from_millis(20);
        // subscription/similar hang (429 from the shared app) — the page does not wait for them longer than grace
        let (page, extras) = with_extras(async { "page" }, std::future::pending::<bool>(), grace).await;
        assert_eq!((page, extras), ("page", None));
        // the ones that made it — get on the page, even if they answered later than the main read
        let late = async {
            tokio::time::sleep(Duration::from_millis(5)).await;
            true
        };
        assert_eq!(with_extras(async { "page" }, late, grace).await, ("page", Some(true)));
        // the main read is longer — it is waited for entirely
        let slow = async {
            tokio::time::sleep(Duration::from_millis(40)).await;
            "page"
        };
        assert_eq!(with_extras(slow, async { true }, grace).await, ("page", Some(true)));
    }
}
