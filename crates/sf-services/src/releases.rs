//! "New releases": Spotify's personal "Release Radar" plus fresh albums and singles by artists the
//! user follows (and by those they listen to a lot in this application).
//!
//! Everything is read through a librespot session (sf_player::engine::new_releases): the Web API's shared key hits 429.
//! The "Radar" is Spotify home (homeview) and the "Made for you" page (GraphQL) through the session → the "Made For You"
//! category by the shared key (in the background, with a timeout) → a playlist from the library if the user saved it. LibraryService then reads its rows, as for any
//! playlist. New releases are follows from the collection, a record for each artist (no more than CONCURRENCY at once) and
//! release dates in extended-metadata batches. Dates of already seen releases are stored on disk, so a repeated pass
//! asks for details only about new ones. Without a session — a time-limited fallback pass over the Web API: 429 and
//! the quota stop it, but do not take the page down. The result is on disk per account: the page and the home shelf are visible
//! at once, the network — no more often than once per REFRESH_EVERY without force.
use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use futures::StreamExt as _;
use jiff::ToSpan as _;
use jiff::civil::Date;
use md3_model::{AlbumItem, PlaylistItem};
use serde::{Deserialize, Serialize};
use sf_api::models::{Album, Page};
use sf_api::{ApiClient, ApiError, ApiGateway, Operation};
use sf_player::engine::new_releases::{self, ReleaseRef};
use sf_player::engine::session_reads;
use sf_player::{PlayerHandle, Session};
use tokio::runtime::Handle;
use tokio::sync::{broadcast, watch};

use crate::ServiceError;
use crate::auth::{AuthEvent, AuthService};
use crate::data::home_feed::{self, FeedCard};
use crate::data::{fsio, route};
use crate::history::HistoryService;
use crate::library::{LibraryEvent, LibraryService};
use crate::lock;

/// No more often than once in 6 hours without force: releases come out on Fridays, the "Radar" updates once a week.
pub const REFRESH_EVERY: Duration = Duration::from_secs(6 * 3600);
/// The freshness window, in days.
pub const WINDOW_DAYS: i64 = 42;
/// How many artists to walk (follows first, then frequently listened).
pub const MAX_ARTISTS: usize = 150;
/// Releases on the page and in the cache.
pub const MAX_RELEASES: usize = 60;
/// Frequently listened artists from the local history.
const FREQUENT_ARTISTS: usize = 20;
const HISTORY_PLAYS: usize = 400;
/// Concurrent artist record reads; detail batches — fewer (they are larger).
const CONCURRENCY: usize = 4;
const BATCH_CONCURRENCY: usize = 2;
/// One read through the session.
const READ_TIMEOUT: Duration = Duration::from_secs(20);
/// Fallback Web API requests (the shared key may answer 429 for hours — it cannot be waited for).
const WEB_TIMEOUT: Duration = Duration::from_secs(15);
/// The whole fallback Web API pass.
const WEB_CRAWL_TIMEOUT: Duration = Duration::from_secs(90);
/// Artists in the fallback pass (each — 1–2 Web API requests).
const WEB_MAX_ARTISTS: usize = 40;
const FOLLOWED_PAGES: usize = 4;
const ALBUMS_PER_ARTIST: u32 = 20;
const SINGLES_PER_ARTIST: u32 = 10;
/// After sign-in, wait for the library and home to load.
const START_DELAY: Duration = Duration::from_secs(5);
const RADAR_TIMEOUT: Duration = Duration::from_secs(30);
/// How long to wait for the session while the librespot engine connects.
const SESSION_WAIT: Duration = Duration::from_secs(20);
const CACHE_VERSION: u32 = 1;

/// A fresh release: AlbumItem (tile keys) + date.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NewRelease {
    #[serde(flatten)]
    pub album: AlbumItem,
    /// Spotify release_date: "2026-10-03" or "2026-10"
    pub release_date: String,
    /// "Today", "Yesterday", "October 3" — recalculated at each publication
    pub date_label: String,
    /// album | single — the group at the artist
    pub group: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReleasesSnapshot {
    pub loading: bool,
    /// the personal "Release Radar"; None — not found or still being looked for
    pub radar: Option<PlaylistItem>,
    /// the "Radar" search is finished (otherwise the page shows loading, not "not found")
    pub radar_checked: bool,
    pub releases: Arc<Vec<NewRelease>>,
    pub error_text: String,
}

#[derive(Default)]
struct State {
    account: String,
    loaded_at: Option<Instant>,
    radar: Option<PlaylistItem>,
    /// the session answered for the "Radar" (personal for sure); otherwise we ask again once the session is ready
    radar_from_session: bool,
    radar_checked: bool,
    releases: Vec<NewRelease>,
    /// release id → release_date of every seen release (details are asked only about new ones)
    known: HashMap<String, String>,
    /// the last pass went through the session (otherwise, when it appears, walk again)
    via_session: bool,
    /// the session came up during a pass without it — walk again once it finishes
    session_pending: bool,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Cached {
    version: u32,
    account: String,
    /// unix load time, ms
    fetched_at: i64,
    radar: Option<PlaylistItem>,
    radar_from_session: bool,
    releases: Vec<NewRelease>,
    known: HashMap<String, String>,
    /// the pass went through the session: after sign-in do not repeat it when the session comes up
    via_session: bool,
}

/// The result of a pass: releases, dates of seen ones and whether it went through the session.
struct Crawl {
    releases: Vec<NewRelease>,
    known: HashMap<String, String>,
    via_session: bool,
}

pub struct ReleasesService {
    rt: Handle,
    auth: Arc<AuthService>,
    library: Arc<LibraryService>,
    history: Arc<HistoryService>,
    player: PlayerHandle,
    http: sf_api::Http,
    snapshot: watch::Sender<ReleasesSnapshot>,
    state: Mutex<State>,
    generation: AtomicU64,
    radar_busy: AtomicBool,
    me: Weak<ReleasesService>,
}

impl ReleasesService {
    pub fn new(
        rt: Handle,
        auth: Arc<AuthService>,
        library: Arc<LibraryService>,
        history: Arc<HistoryService>,
        player: PlayerHandle,
        http: sf_api::Http,
    ) -> Arc<Self> {
        let service = Arc::new_cyclic(|me| Self {
            rt: rt.clone(),
            auth,
            library,
            history,
            player,
            http,
            snapshot: watch::channel(ReleasesSnapshot::default()).0,
            state: Mutex::new(State::default()),
            generation: AtomicU64::new(0),
            radar_busy: AtomicBool::new(false),
            me: me.clone(),
        });
        rt.spawn(listen(Arc::downgrade(&service)));
        service
    }

    /// Refresh (force — without the 6-hour limit).
    pub fn refresh(&self, force: bool) {
        if !self.auth.is_signed_in() {
            self.snapshot.send_replace(ReleasesSnapshot {
                error_text: ServiceError::NotSignedIn.to_string(),
                ..Default::default()
            });
            return;
        }
        {
            let mut state = lock(&self.state);
            let fresh = state.loaded_at.is_some_and(|at| at.elapsed() < REFRESH_EVERY);
            if fresh && !force {
                return;
            }
            state.loaded_at = Some(Instant::now());
            state.session_pending = false;
        }
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.snapshot.send_modify(|s| {
            s.loading = true;
            s.error_text.clear();
        });
        let Some(me) = self.me.upgrade() else { return };
        self.rt.spawn(async move { me.load(generation).await });
    }

    pub fn snapshot(&self) -> ReleasesSnapshot {
        self.snapshot.borrow().clone()
    }

    pub fn watch(&self) -> watch::Receiver<ReleasesSnapshot> {
        self.snapshot.subscribe()
    }

    // --- internals -----------------------------------------------------------------------------

    fn current(&self, generation: u64) -> bool {
        self.generation.load(Ordering::SeqCst) == generation
    }

    fn publish(&self) {
        let today = jiff::Zoned::now().date();
        let (radar, radar_checked, releases) = {
            let state = lock(&self.state);
            (
                state.radar.clone(),
                state.radar_checked,
                labelled(&state.releases, today),
            )
        };
        self.snapshot.send_if_modified(|s| {
            if s.radar == radar && s.radar_checked == radar_checked && *s.releases == releases {
                return false;
            }
            s.radar = radar;
            s.radar_checked = radar_checked;
            s.releases = Arc::new(releases);
            true
        });
    }

    fn store(&self, generation: u64, apply: impl FnOnce(&mut State)) {
        if !self.current(generation) {
            return;
        }
        apply(&mut lock(&self.state));
        self.publish();
    }

    async fn load(self: Arc<Self>, generation: u64) {
        let (radar, from_session) = self.discover_radar().await;
        self.store(generation, |s| {
            s.radar = radar;
            s.radar_from_session = from_session;
            s.radar_checked = true;
        });
        let crawled = self.crawl().await;
        if !self.current(generation) {
            return;
        }
        let error = match crawled {
            Ok(crawl) => {
                self.store(generation, |s| {
                    s.releases = crawl.releases;
                    s.known = crawl.known;
                    s.via_session = crawl.via_session;
                });
                None
            }
            Err(error) => {
                tracing::debug!("new releases: {error}");
                // a failure does not wait 6 hours: the next page open will ask again
                lock(&self.state).loaded_at = None;
                Some(error)
            }
        };
        self.save_cache();
        let empty = lock(&self.state).releases.is_empty();
        self.snapshot.send_modify(|s| {
            s.loading = false;
            // the error is visible only if there is nothing to show
            s.error_text = error.filter(|_| empty).unwrap_or_default();
        });
        // the session came up while a pass without it was running — walk again through it (no Web API quota)
        let again = {
            let mut state = lock(&self.state);
            std::mem::take(&mut state.session_pending) && !state.via_session
        };
        if again && self.session().is_some() {
            self.refresh(true);
        }
    }

    fn session(&self) -> Option<Session> {
        route::account_session(&self.auth.api(), &self.player)
    }

    /// A session, waiting for it up to SESSION_WAIT: after sign-in the librespot engine does not come up at once, and without it
    /// only the Web API is left.
    async fn wait_session(&self) -> Option<Session> {
        let started = Instant::now();
        loop {
            if let Some(session) = self.session() {
                return Some(session);
            }
            let local = self.player.local_playback();
            let coming = matches!(local, sf_player::LocalPlayback::Connecting);
            if !coming || started.elapsed() >= SESSION_WAIT {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// The session is ready: repeat a pass without it (Web API) through it, otherwise — the "Radar" only. A running pass
    /// is not duplicated: it repeats once it finishes, if it went without the session after all.
    fn session_ready(&self) {
        let loading = self.snapshot.borrow().loading;
        let again = {
            let mut state = lock(&self.state);
            let again = !state.account.is_empty() && state.loaded_at.is_some() && !state.via_session;
            if again && loading {
                state.session_pending = true;
            }
            again
        };
        if again && !loading {
            self.refresh(true);
        } else {
            self.recheck_radar();
        }
    }

    /// The "Radar" and whether the session answered: Spotify home (homeview) → the "Made for you" page (GraphQL) through the
    /// session → the Web API shared key (in the background) → the library.
    async fn discover_radar(&self) -> (Option<PlaylistItem>, bool) {
        let api = self.auth.api();
        let me = self.auth.account_id();
        let map = |p: &sf_api::models::Playlist| sf_api::map::playlist_item(p, me.as_ref());
        let mut session_answered = false;
        if let Some(session) = self.session() {
            match tokio::time::timeout(RADAR_TIMEOUT, new_releases::release_radar(&session)).await {
                Ok(Ok(Some(radar))) => return (Some(map(&radar)), true),
                Ok(Ok(None)) => session_answered = true,
                Ok(Err(error)) => tracing::debug!("Release Radar on the Spotify home: {error:#}"),
                Err(_) => tracing::debug!("Release Radar on the Spotify home: timed out"),
            }
            match tokio::time::timeout(RADAR_TIMEOUT, browse_radar(&self.http, &session)).await {
                Ok(Ok(Some(radar))) => return (Some(map(&radar)), true),
                Ok(Ok(None)) => session_answered = true,
                Ok(Err(error)) => tracing::debug!("Release Radar on the “Made for you” page: {error:#}"),
                Err(_) => tracing::debug!("Release Radar on the “Made for you” page: timed out"),
            }
        }
        if !session_answered {
            match sf_api::limits::background(within(WEB_TIMEOUT, shared_radar(&api))).await {
                Ok(Some(radar)) => return (Some(map(&radar)), false),
                Ok(None) => {}
                Err(error) => tracing::debug!("Release Radar with the shared key: {error}"),
            }
        }
        (radar_in_library(&self.library.playlists()), session_answered)
    }

    /// The "Radar" only (the session became ready later than sign-in).
    fn recheck_radar(&self) {
        {
            let state = lock(&self.state);
            if state.radar_from_session || !state.radar_checked || state.account.is_empty() {
                return;
            }
        }
        if self.radar_busy.swap(true, Ordering::SeqCst) {
            return;
        }
        let Some(me) = self.me.upgrade() else {
            self.radar_busy.store(false, Ordering::SeqCst);
            return;
        };
        let generation = self.generation.load(Ordering::SeqCst);
        self.rt.spawn(async move {
            let (radar, from_session) = me.discover_radar().await;
            me.radar_busy.store(false, Ordering::SeqCst);
            if radar.is_none() && !from_session {
                return;
            }
            me.store(generation, |s| {
                s.radar = radar;
                s.radar_from_session = from_session;
            });
            me.save_cache();
        });
    }

    /// Whose releases to look at: follows (session collection → library → Web API), then frequently listened.
    /// Err — the follows cannot be found out any way and there is no history: "nothing new" would be untrue.
    async fn artist_ids(&self, session: Option<&Session>) -> Result<Vec<String>, String> {
        let mut followed = Vec::new();
        let mut answered = false;
        if let Some(session) = session {
            match tokio::time::timeout(READ_TIMEOUT, new_releases::followed_artists(session)).await {
                Ok(Ok(ids)) => {
                    answered = true;
                    followed = ids;
                }
                Ok(Err(error)) => tracing::debug!("new releases: follows via session: {error:#}"),
                Err(_) => tracing::debug!("new releases: follows via session timed out"),
            }
        }
        if followed.is_empty() {
            followed = self.library.artists().into_iter().map(|a| a.artist_id).collect();
        }
        // the session answered "not following anyone" — the Web API will say the same
        if followed.is_empty() && !answered {
            match sf_api::limits::background(within(WEB_TIMEOUT, followed_artists(&self.auth.api()))).await {
                Ok(ids) => {
                    answered = true;
                    followed = ids;
                }
                Err(error) => tracing::debug!("new releases: follows via Web API: {error}"),
            }
        }
        let plays: Vec<Vec<String>> = self
            .history
            .recent(HISTORY_PLAYS)
            .into_iter()
            .map(|play| play.track.artist_refs.into_iter().map(|a| a.id).collect())
            .collect();
        let pool = artist_pool(&followed, &frequent_artists(&plays, FREQUENT_ARTISTS), MAX_ARTISTS);
        if pool.is_empty() && !answered {
            return Err(mf_i18n::t("Spotify didn’t return the artists you follow — new releases will update later"));
        }
        Ok(pool)
    }

    async fn crawl(&self) -> Result<Crawl, String> {
        let session = self.wait_session().await;
        let pool = self.artist_ids(session.as_ref()).await?;
        if pool.is_empty() {
            return Ok(Crawl {
                releases: Vec::new(),
                known: HashMap::new(),
                via_session: session.is_some(),
            });
        }
        let (known, previous) = {
            let state = lock(&self.state);
            (state.known.clone(), state.releases.clone())
        };
        let today = jiff::Zoned::now().date();
        match session {
            Some(session) => session_crawl(&session, &pool, known, &previous, today).await,
            None => {
                let api = self.auth.api();
                // a background pass: yields to pages and does not hit the network while the Web API is limited
                let crawl = sf_api::limits::background(web_crawl(&api, &pool, today));
                match tokio::time::timeout(WEB_CRAWL_TIMEOUT, crawl).await {
                    Ok(result) => result
                        .map(|releases| Crawl {
                            releases,
                            known,
                            via_session: false,
                        })
                        .map_err(|error| ServiceError::from(error).to_string()),
                    Err(_) => Err(mf_i18n::t("Spotify is taking too long to respond — new releases will update later")),
                }
            }
        }
    }

    // --- disk cache -----------------------------------------------------------------------------

    fn cache_file(account: &str) -> PathBuf {
        let safe: String = account
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
            .collect();
        mf_platform::dirs::cache_subdir("releases").join(format!("{safe}.json"))
    }

    fn save_cache(&self) {
        let cache = {
            let state = lock(&self.state);
            if state.account.is_empty() {
                return;
            }
            Cached {
                version: CACHE_VERSION,
                account: state.account.clone(),
                fetched_at: jiff::Timestamp::now().as_millisecond(),
                radar: state.radar.clone(),
                radar_from_session: state.radar_from_session,
                releases: state.releases.clone(),
                known: state.known.clone(),
                via_session: state.via_session,
            }
        };
        let path = Self::cache_file(&cache.account);
        tokio::task::spawn_blocking(move || match serde_json::to_vec(&cache) {
            Ok(bytes) => {
                if let Err(error) = fsio::write_atomic(&path, &bytes) {
                    tracing::debug!("new releases cache not written: {error:#}");
                }
            }
            Err(error) => tracing::debug!("new releases cache not written: {error:#}"),
        });
    }

    /// Sign-in: show this account's cache and refresh it if it is old.
    async fn signed_in(self: Arc<Self>, account: String) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        *lock(&self.state) = State {
            account: account.clone(),
            ..Default::default()
        };
        // the "Sign in to your account" from the page opened before sign-in is no longer true
        self.snapshot.send_replace(ReleasesSnapshot::default());
        let path = Self::cache_file(&account);
        let cached = tokio::task::spawn_blocking(move || std::fs::read(path).ok())
            .await
            .ok()
            .flatten()
            .and_then(|bytes| serde_json::from_slice::<Cached>(&bytes).ok())
            .filter(|c| c.version == CACHE_VERSION && c.account == account);
        if let Some(cached) = cached {
            let age_ms = (jiff::Timestamp::now().as_millisecond() - cached.fetched_at).max(0);
            let age = Duration::from_millis(u64::try_from(age_ms).unwrap_or(u64::MAX));
            let mut state = lock(&self.state);
            if state.account == account {
                state.loaded_at = Instant::now().checked_sub(age);
                state.radar = cached.radar;
                state.radar_from_session = cached.radar_from_session;
                state.radar_checked = true;
                state.releases = cached.releases;
                state.known = cached.known;
                state.via_session = cached.via_session;
            }
        }
        self.publish();
        tokio::time::sleep(START_DELAY).await;
        if lock(&self.state).account == account {
            self.refresh(false);
        }
    }

    fn clear(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        *lock(&self.state) = State::default();
        self.snapshot.send_replace(ReleasesSnapshot::default());
    }
}

/// Sign-in/sign-out, session readiness and the library → the "Radar" and new releases.
async fn listen(weak: Weak<ReleasesService>) {
    let Some(me) = weak.upgrade() else { return };
    let mut auth = me.auth.subscribe();
    let mut library = me.library.subscribe();
    drop(me);
    loop {
        tokio::select! {
            event = auth.recv() => {
                let Some(me) = weak.upgrade() else { break };
                match event {
                    Ok(AuthEvent::SignedIn(account)) => {
                        me.rt.clone().spawn(me.signed_in(account.id));
                    }
                    Ok(AuthEvent::SignedOut) => me.clear(),
                    Ok(AuthEvent::PlaybackReady(_)) => me.session_ready(),
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            },
            event = library.recv() => match event {
                Ok(LibraryEvent::Playlists(_)) => {
                    let Some(me) = weak.upgrade() else { break };
                    // the saved "Radar" from the library — only if it was not found otherwise
                    let missing = lock(&me.state).radar.is_none();
                    if missing && let Some(radar) = radar_in_library(&me.library.playlists()) {
                        lock(&me.state).radar = Some(radar);
                        me.publish();
                    }
                }
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            },
        }
    }
}

// --- network: session ------------------------------------------------------------------------------

/// A pass over `artists` through the session without the date cache (a check on a live account, debugging).
pub async fn crawl_session(session: &Session, artists: &[String]) -> Result<Vec<NewRelease>, String> {
    let today = jiff::Zoned::now().date();
    session_crawl(session, artists, HashMap::new(), &[], today).await.map(|crawl| crawl.releases)
}

/// A pass through the session: artist records → details of new releases in batches → fresh ones.
async fn session_crawl(
    session: &Session,
    pool: &[String],
    mut known: HashMap<String, String>,
    previous: &[NewRelease],
    today: Date,
) -> Result<Crawl, String> {
    // futures own their data (the session is an Arc inside): otherwise the tokio task is not Send
    let answers: Vec<(usize, Option<Vec<ReleaseRef>>)> = futures::stream::iter(pool.iter().cloned().enumerate())
        .map(|(rank, artist_id)| {
            let session = session.clone();
            async move {
                match tokio::time::timeout(READ_TIMEOUT, new_releases::artist_releases(&session, &artist_id)).await {
                    Ok(Ok(refs)) => (rank, Some(refs)),
                    Ok(Err(error)) => {
                        tracing::debug!("new releases: artist {artist_id}: {error:#}");
                        (rank, None)
                    }
                    Err(_) => (rank, None),
                }
            }
        })
        .buffer_unordered(CONCURRENCY)
        .collect()
        .await;
    if answers.iter().all(|(_, refs)| refs.is_none()) {
        return Err(mf_i18n::t("Spotify didn’t respond — new releases will update later"));
    }
    let refs = best_refs(answers.into_iter().filter_map(|(rank, refs)| Some((rank, refs?))));

    let mut details: HashMap<String, NewRelease> =
        previous.iter().map(|r| (r.album.album_id.clone(), r.clone())).collect();
    let wanted = needs_details(&refs, &known, &details, today, WINDOW_DAYS);
    let batches: Vec<Vec<ReleaseRef>> = wanted.chunks(new_releases::RELEASE_BATCH).map(<[_]>::to_vec).collect();
    let fetched: Vec<Vec<Album>> = futures::stream::iter(batches)
        .map(|batch| {
            let session = session.clone();
            async move {
                match tokio::time::timeout(READ_TIMEOUT, new_releases::release_details(&session, &batch)).await {
                    Ok(Ok(albums)) => albums,
                    Ok(Err(error)) => {
                        tracing::debug!("new releases: release details: {error:#}");
                        Vec::new()
                    }
                    Err(_) => Vec::new(),
                }
            }
        })
        .buffer_unordered(BATCH_CONCURRENCY)
        .collect()
        .await;
    for album in fetched.into_iter().flatten() {
        let release = release_of(&album);
        known.insert(release.album.album_id.clone(), release.release_date.clone());
        details.insert(release.album.album_id.clone(), release);
    }
    // we keep dates only for releases still listed under artists
    known.retain(|id, _| refs.contains_key(id));
    let candidates = refs
        .iter()
        .filter_map(|(id, (rank, _))| Some((*rank, details.get(id)?.clone())));
    Ok(Crawl {
        releases: select(candidates.collect(), today, WINDOW_DAYS, MAX_RELEASES),
        known,
        via_session: true,
    })
}

/// Releases of every artist: id → (the artist's best position, group).
fn best_refs(answers: impl Iterator<Item = (usize, Vec<ReleaseRef>)>) -> HashMap<String, (usize, &'static str)> {
    let mut best: HashMap<String, (usize, &'static str)> = HashMap::new();
    for (rank, refs) in answers {
        for release in refs {
            let entry = best.entry(release.album_id).or_insert((rank, release.group));
            if rank < entry.0 {
                *entry = (rank, release.group);
            }
        }
    }
    best
}

/// Which releases to ask details about: the date is unknown, or the release is fresh and there is nothing to show it with.
fn needs_details(
    refs: &HashMap<String, (usize, &'static str)>,
    known: &HashMap<String, String>,
    details: &HashMap<String, NewRelease>,
    today: Date,
    window_days: i64,
) -> Vec<ReleaseRef> {
    let mut wanted: Vec<(usize, ReleaseRef)> = refs
        .iter()
        .filter(|(id, _)| match known.get(*id) {
            None => true,
            Some(date) => {
                let fresh = release_day(date).is_some_and(|day| is_fresh(day, today, window_days));
                fresh && !details.contains_key(*id)
            }
        })
        .map(|(id, (rank, group))| {
            (
                *rank,
                ReleaseRef {
                    album_id: id.clone(),
                    group,
                },
            )
        })
        .collect();
    // higher artists' releases first: a cut-short pass loses the less important
    wanted.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.album_id.cmp(&b.1.album_id)));
    wanted.into_iter().map(|(_, release)| release).collect()
}

// --- network: Web API (fallback path) --------------------------------------------------------------

/// A Web API request that must not hold the page: longer than `limit` — like a network error.
async fn within<T>(limit: Duration, request: impl Future<Output = Result<T, ApiError>>) -> Result<T, ApiError> {
    tokio::time::timeout(limit, request)
        .await
        .unwrap_or_else(|_| Err(ApiError::Network("Spotify didn’t respond in time".into())))
}

/// Spotify's personal playlists ("Radar", Discover Weekly) — their label is finished reading if it is missing.
const PERSONAL_PLAYLIST: &str = "37i9dQZEVXb";

/// The "Radar" on the "Made for you" page (GraphQL browsePage through the session, no Web API quota). Mix labels
/// do not always arrive — only personal playlists without a name get finished. Ok(None) — the page answered,
/// but there is no "Radar" on it.
async fn browse_radar(http: &sf_api::Http, session: &Session) -> anyhow::Result<Option<sf_api::models::Playlist>> {
    let cards: Vec<FeedCard> = home_feed::made_for_you(http, session)
        .await?
        .into_iter()
        .flat_map(|section| section.cards)
        .filter(|card| card.kind() == "playlist")
        .collect();
    let (unnamed, mut named): (Vec<FeedCard>, Vec<FeedCard>) =
        cards.into_iter().partition(|card| card.title.is_empty() && card.id().starts_with(PERSONAL_PLAYLIST));
    named.extend(home_feed::resolve_playlists(session, unnamed).await);
    let playlists: Vec<sf_api::models::Playlist> = named.iter().map(card_playlist).collect();
    let Some(radar) = new_releases::pick_release_radar(&playlists) else { return Ok(None) };
    // the full header (cover, description, track count) — through the session; if it fails a card will do
    match session_reads::playlist(session, &radar.id).await {
        Ok(playlist) => Ok(Some(playlist)),
        Err(_) => Ok(Some(radar.clone())),
    }
}

/// A "Made for you" page card as a playlist header.
fn card_playlist(card: &FeedCard) -> sf_api::models::Playlist {
    let mut playlist = sf_api::models::Playlist {
        id: card.id().to_owned(),
        name: card.title.clone(),
        uri: card.uri.clone(),
        description: (!card.subtitle.is_empty()).then(|| card.subtitle.clone()),
        ..Default::default()
    };
    if !card.cover.is_empty() {
        playlist.images.push(sf_api::models::Image { url: card.cover.clone(), width: None, height: None });
    }
    playlist.owner.id = (!card.owner_id.is_empty()).then(|| card.owner_id.clone());
    playlist.owner.display_name = (!card.owner.is_empty()).then(|| card.owner.clone());
    playlist
}

async fn shared_radar(api: &ApiGateway) -> Result<Option<sf_api::models::Playlist>, ApiError> {
    let client = api.client_for(Operation::PlaylistSearch).await?;
    let page = client
        .category_playlists(new_releases::MADE_FOR_YOU_CATEGORY, 0, new_releases::CATEGORY_LIMIT)
        .await?;
    Ok(new_releases::pick_release_radar(&page.items).cloned())
}

async fn followed_artists(api: &ApiGateway) -> Result<Vec<String>, ApiError> {
    let client = api.client_for(Operation::UserData).await?;
    let mut artists = Vec::new();
    let mut after: Option<String> = None;
    for _ in 0..FOLLOWED_PAGES {
        let page = client.followed_artists(after.as_deref(), 50).await?;
        artists.extend(page.items.iter().map(|a| a.id.clone()));
        after = page.cursors.and_then(|c| c.after).filter(|a| !a.is_empty());
        if after.is_none() || page.next.is_none() {
            break;
        }
    }
    Ok(artists)
}

/// Without a session: /artists/{id}/albums for the first WEB_MAX_ARTISTS artists. A long 429, the quota or sign-in
/// stops the pass; whatever we managed — we show.
async fn web_crawl(api: &ApiGateway, pool: &[String], today: Date) -> Result<Vec<NewRelease>, ApiError> {
    let client = api.client_for(Operation::Catalog).await?;
    let stop = Arc::new(AtomicBool::new(false));
    let answers: Vec<Result<Vec<(usize, NewRelease)>, ApiError>> =
        futures::stream::iter(pool.iter().take(WEB_MAX_ARTISTS).cloned().enumerate())
            .map(|(rank, artist_id)| {
                let client = client.clone();
                let stop = stop.clone();
                async move {
                    if stop.load(Ordering::Relaxed) {
                        return Ok(Vec::new());
                    }
                    match web_artist_releases(&client, &artist_id).await {
                        Ok(albums) => Ok(albums.iter().map(|album| (rank, release_of(album))).collect()),
                        Err(error) => {
                            if stops_the_crawl(&error) {
                                stop.store(true, Ordering::Relaxed);
                            }
                            Err(error)
                        }
                    }
                }
            })
            .buffer_unordered(CONCURRENCY)
            .collect()
            .await;
    let mut candidates = Vec::new();
    let mut answered = 0;
    let mut first_error = None;
    for answer in answers {
        match answer {
            Ok(found) => {
                answered += 1;
                candidates.extend(found);
            }
            Err(error) => {
                tracing::debug!("new releases: artist releases via Web API: {error}");
                first_error.get_or_insert(error);
            }
        }
    }
    if answered == 0
        && let Some(error) = first_error
    {
        return Err(error);
    }
    Ok(select(candidates, today, WINDOW_DAYS, MAX_RELEASES))
}

/// An artist's albums and singles, newest first in each group. Spotify returns albums first: if the page
/// ran out on them, singles are asked for separately.
async fn web_artist_releases(client: &ApiClient, artist_id: &str) -> Result<Vec<Album>, ApiError> {
    let page = client
        .artist_albums(artist_id, "album,single", 0, ALBUMS_PER_ARTIST)
        .await?;
    let more_singles = singles_cut_off(&page);
    let mut albums = page.items;
    if more_singles {
        match client.artist_albums(artist_id, "single", 0, SINGLES_PER_ARTIST).await {
            Ok(singles) => albums.extend(singles.items),
            Err(error) => tracing::debug!("new releases: singles {artist_id}: {error}"),
        }
    }
    Ok(albums)
}

/// Asking further is pointless: the quota, a long 429 or sign-in.
fn stops_the_crawl(error: &ApiError) -> bool {
    matches!(
        error,
        ApiError::RateLimited | ApiError::QuotaExhausted | ApiError::NotSignedIn | ApiError::SignInExpired { .. }
    )
}

// --- pure functions ------------------------------------------------------------------------------

/// The "Radar" saved in the library.
pub fn radar_in_library(playlists: &[PlaylistItem]) -> Option<PlaylistItem> {
    playlists
        .iter()
        .find(|p| new_releases::is_release_radar(&p.playlist_id, &p.title, &p.owner_id))
        .cloned()
}

/// Artists from the listening history (by tracks, each a list of ids), most listened first; ≤ max.
pub fn frequent_artists(plays: &[Vec<String>], max: usize) -> Vec<String> {
    let mut counts: HashMap<&str, (usize, usize)> = HashMap::new();
    for (order, artists) in plays.iter().enumerate() {
        for id in artists.iter().filter(|id| !id.is_empty()) {
            counts.entry(id.as_str()).or_insert((0, order)).0 += 1;
        }
    }
    let mut ranked: Vec<(&str, (usize, usize))> = counts.into_iter().collect();
    // more often — higher; equal — whoever was listened to later (history is newest first)
    ranked.sort_by(|a, b| b.1.0.cmp(&a.1.0).then(a.1.1.cmp(&b.1.1)));
    ranked.into_iter().take(max).map(|(id, _)| id.to_owned()).collect()
}

/// Whose releases to look at: follows in collection order, then frequently listened; without repeats, ≤ max.
pub fn artist_pool(followed: &[String], frequent: &[String], max: usize) -> Vec<String> {
    let mut seen = HashSet::new();
    followed
        .iter()
        .chain(frequent)
        .map(String::as_str)
        .filter(|id| !id.is_empty() && seen.insert(*id))
        .take(max)
        .map(str::to_owned)
        .collect()
}

fn group_of(album: &Album) -> &str {
    album
        .album_group
        .as_deref()
        .or(album.album_type.as_deref())
        .unwrap_or("album")
}

/// The page ran out and there are no singles in it — they are past its edge.
pub fn singles_cut_off(page: &Page<Album>) -> bool {
    page.total as usize > page.items.len() && !page.items.iter().any(|a| group_of(a) == "single")
}

/// A release as a tile (the date label — at publication).
pub fn release_of(album: &Album) -> NewRelease {
    NewRelease {
        album: sf_api::map::album_item(album),
        release_date: album.release_date.clone().unwrap_or_default(),
        date_label: String::new(),
        group: group_of(album).to_owned(),
    }
}

/// The release day from release_date: "2026-10-03" — itself, "2026-10" — the first; a year alone — unknown.
pub fn release_day(date: &str) -> Option<Date> {
    let mut parts = date.trim().splitn(3, '-');
    let year: i16 = parts.next()?.parse().ok()?;
    let month: i8 = parts.next()?.parse().ok()?;
    let day: i8 = match parts.next() {
        Some(day) => day.parse().ok()?,
        None => 1,
    };
    Date::new(year, month, day).ok()
}

/// Came out in the last `window_days` days (not in the future: those are not listened to yet).
pub fn is_fresh(day: Date, today: Date, window_days: i64) -> bool {
    let Ok(since) = today.checked_sub(window_days.days()) else {
        return false;
    };
    since <= day && day <= today
}

/// "{0}" — the day of the month (the dictionary key carries the month name: "{0} January").
const MONTH_DAY: [&str; 12] = [
    "January {0}",
    "February {0}",
    "March {0}",
    "April {0}",
    "May {0}",
    "June {0}",
    "July {0}",
    "August {0}",
    "September {0}",
    "October {0}",
    "November {0}",
    "December {0}",
];

/// The same with the year: "{0}" — the day, "{1}" — the year.
const MONTH_DAY_YEAR: [&str; 12] = [
    "January {0}, {1}",
    "February {0}, {1}",
    "March {0}, {1}",
    "April {0}, {1}",
    "May {0}, {1}",
    "June {0}, {1}",
    "July {0}, {1}",
    "August {0}, {1}",
    "September {0}, {1}",
    "October {0}, {1}",
    "November {0}, {1}",
    "December {0}, {1}",
];

/// "Today", "Yesterday", "October 3", in another year — "December 28, 2025" (each language translates these keys with its own word order).
pub fn date_label(day: Date, today: Date) -> String {
    if day == today {
        return mf_i18n::t("Today");
    }
    if today.yesterday().ok() == Some(day) {
        return mf_i18n::t("Yesterday");
    }
    let month = (day.month() - 1).clamp(0, 11) as usize;
    if day.year() == today.year() {
        mf_i18n::tf(MONTH_DAY[month], &[&day.day()])
    } else {
        mf_i18n::tf(MONTH_DAY_YEAR[month], &[&day.day(), &day.year()])
    }
}

/// Date labels for today.
fn labelled(releases: &[NewRelease], today: Date) -> Vec<NewRelease> {
    releases
        .iter()
        .map(|r| NewRelease {
            date_label: release_day(&r.release_date)
                .map(|d| date_label(d, today))
                .unwrap_or_default(),
            ..r.clone()
        })
        .collect()
}

/// Fresh releases (rank — the artist's position in the selection): without repeats (a release by two artists — under the one who
/// is higher), newest first, on an equal date — by the artist's position; ≤ limit.
pub fn select(found: Vec<(usize, NewRelease)>, today: Date, window_days: i64, limit: usize) -> Vec<NewRelease> {
    let mut best: HashMap<String, (Date, usize, NewRelease)> = HashMap::new();
    for (rank, release) in found {
        if release.album.album_id.is_empty() || !matches!(release.group.as_str(), "album" | "single") {
            continue;
        }
        let Some(day) = release_day(&release.release_date) else {
            continue;
        };
        if !is_fresh(day, today, window_days) {
            continue;
        }
        match best.get(&release.album.album_id) {
            Some((_, kept, _)) if *kept <= rank => {}
            _ => {
                best.insert(release.album.album_id.clone(), (day, rank, release));
            }
        }
    }
    let mut picked: Vec<(Date, usize, NewRelease)> = best.into_values().collect();
    picked.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then(a.1.cmp(&b.1))
            .then_with(|| a.2.album.title.cmp(&b.2.album.title))
    });
    picked.truncate(limit);
    let releases: Vec<NewRelease> = picked.into_iter().map(|(_, _, release)| release).collect();
    labelled(&releases, today)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(text: &str) -> Date {
        text.parse().unwrap()
    }

    fn album(id: &str, group: &str, released: &str) -> Album {
        Album {
            id: id.into(),
            name: format!("Album {id}"),
            uri: format!("spotify:album:{id}"),
            album_group: Some(group.into()),
            album_type: Some(group.into()),
            total_tracks: Some(if group == "single" { 1 } else { 10 }),
            release_date: Some(released.into()),
            ..Default::default()
        }
    }

    fn found(rank: usize, id: &str, group: &str, released: &str) -> (usize, NewRelease) {
        (rank, release_of(&album(id, group, released)))
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn release_dates_read_with_their_precision() {
        assert_eq!(release_day("2026-10-03"), Some(date("2026-10-03")));
        assert_eq!(release_day("2026-10"), Some(date("2026-10-01")), "month: its first day");
        assert_eq!(release_day("2026"), None, "a year alone says nothing about freshness");
        assert_eq!(release_day("2026-13-01"), None);
        assert_eq!(release_day(""), None);
    }

    #[test]
    fn only_releases_inside_the_window_are_fresh() {
        let today = date("2026-10-11");
        assert!(is_fresh(today, today, 42));
        assert!(is_fresh(date("2026-08-30"), today, 42), "the window's first day");
        assert!(!is_fresh(date("2026-08-29"), today, 42));
        assert!(!is_fresh(date("2026-10-16"), today, 42), "announced, not out yet");
    }

    #[test]
    fn dates_are_labelled() {
        let today = date("2026-10-11");
        assert_eq!(date_label(today, today), "Today");
        assert_eq!(date_label(date("2026-10-10"), today), "Yesterday");
        assert_eq!(date_label(date("2026-10-03"), today), "October 3");
        assert_eq!(date_label(date("2026-05-01"), today), "May 1");
        assert_eq!(date_label(date("2025-12-28"), date("2026-01-05")), "December 28, 2025");
    }

    #[test]
    fn fresh_releases_come_newest_first_once_each() {
        let today = date("2026-10-11");
        let candidates = vec![
            found(1, "old", "album", "2026-01-10"),
            found(1, "single", "single", "2026-10-09"),
            found(0, "lp", "album", "2026-10-02"),
            found(2, "feat", "appears_on", "2026-10-10"),
            found(2, "lp", "album", "2026-10-02"),
            found(0, "year", "album", "2026"),
            found(3, "same-day-b", "single", "2026-10-09"),
        ];
        let picked = select(candidates, today, 42, 10);
        let ids: Vec<&str> = picked.iter().map(|r| r.album.album_id.as_str()).collect();
        assert_eq!(
            ids,
            ["single", "same-day-b", "lp"],
            "same day: the higher-ranked artist first"
        );
        assert_eq!(picked[0].group, "single");
        assert_eq!(picked[0].date_label, "October 9");
        assert_eq!(picked[2].release_date, "2026-10-02");
        assert_eq!(picked[2].album.uri, "spotify:album:lp");
        assert!(select(vec![found(0, "a", "single", "2026-10-01")], today, 42, 0).is_empty());
    }

    #[test]
    fn followed_artists_come_before_frequent_ones() {
        let pool = artist_pool(&ids(&["a", "b", ""]), &ids(&["b", "c"]), 10);
        assert_eq!(pool, ["a", "b", "c"]);
        assert_eq!(artist_pool(&ids(&["a", "b"]), &ids(&["c"]), 2), ["a", "b"]);
    }

    #[test]
    fn frequent_artists_are_counted_over_the_history() {
        let plays = vec![ids(&["x"]), ids(&["y", "x"]), ids(&["z"]), ids(&["y"]), ids(&["x", ""])];
        assert_eq!(frequent_artists(&plays, 10), ["x", "y", "z"]);
        assert_eq!(frequent_artists(&plays, 1), ["x"]);
        // equal — whoever was listened to later (earlier in the history)
        assert_eq!(
            frequent_artists(&[ids(&["late"]), ids(&["early"])], 2),
            ["late", "early"]
        );
    }

    #[test]
    fn a_release_of_two_artists_counts_for_the_higher_one() {
        let refs = best_refs(
            vec![
                (
                    3,
                    vec![ReleaseRef {
                        album_id: "a".into(),
                        group: "single",
                    }],
                ),
                (
                    1,
                    vec![ReleaseRef {
                        album_id: "a".into(),
                        group: "album",
                    }],
                ),
                (
                    2,
                    vec![ReleaseRef {
                        album_id: "b".into(),
                        group: "album",
                    }],
                ),
            ]
            .into_iter(),
        );
        assert_eq!(refs["a"], (1, "album"));
        assert_eq!(refs["b"], (2, "album"));
    }

    #[test]
    fn only_new_or_undescribed_fresh_releases_are_asked_about() {
        let today = date("2026-10-11");
        let refs = best_refs(
            vec![(
                0,
                ["new", "old", "fresh-shown", "fresh-lost"]
                    .iter()
                    .map(|id| ReleaseRef {
                        album_id: (*id).into(),
                        group: "album",
                    })
                    .collect(),
            )]
            .into_iter(),
        );
        let known: HashMap<String, String> = [
            ("old", "2020-01-01"),
            ("fresh-shown", "2026-10-02"),
            ("fresh-lost", "2026-10-03"),
        ]
        .into_iter()
        .map(|(id, d)| (id.to_owned(), d.to_owned()))
        .collect();
        let details: HashMap<String, NewRelease> = [found(0, "fresh-shown", "album", "2026-10-02").1]
            .into_iter()
            .map(|r| (r.album.album_id.clone(), r))
            .collect();
        let asked: Vec<String> = needs_details(&refs, &known, &details, today, 42)
            .into_iter()
            .map(|r| r.album_id)
            .collect();
        assert_eq!(asked, ["fresh-lost", "new"]);
    }

    #[test]
    fn singles_are_asked_for_only_when_the_page_hid_them() {
        let page = |items: Vec<Album>, total: u32| Page {
            items,
            total,
            ..Default::default()
        };
        let albums = vec![album("a", "album", "2026-01-01"), album("b", "album", "2025-01-01")];
        assert!(singles_cut_off(&page(albums.clone(), 30)));
        assert!(!singles_cut_off(&page(albums.clone(), 2)), "the whole list fit");
        let mixed = vec![album("a", "album", "2026-01-01"), album("s", "single", "2026-02-01")];
        assert!(!singles_cut_off(&page(mixed, 30)));
    }

    #[test]
    fn a_saved_radar_is_found_in_the_library() {
        let mine = PlaylistItem {
            playlist_id: "x".into(),
            title: "Release Radar".into(),
            owner_id: "me".into(),
            ..Default::default()
        };
        let radar = PlaylistItem {
            playlist_id: "37i9dQZEVXbsGPw6v9FjFR".into(),
            title: "Release Radar".into(),
            owner_id: "spotify".into(),
            ..Default::default()
        };
        assert_eq!(radar_in_library(&[mine.clone(), radar.clone()]), Some(radar));
        assert_eq!(radar_in_library(&[mine]), None, "a copy of one's own is not the radar");
    }

    #[test]
    fn a_release_serializes_with_tile_keys_and_its_date() {
        let release = select(vec![found(0, "lp", "album", "2026-10-02")], date("2026-10-11"), 42, 5).remove(0);
        let json = serde_json::to_value(&release).unwrap();
        assert_eq!(json["albumId"], "lp");
        assert_eq!(json["releaseDate"], "2026-10-02");
        assert_eq!(json["dateLabel"], "October 2");
        assert_eq!(json["group"], "album");
        assert!(json["kind"].is_string());
        let back: NewRelease = serde_json::from_value(json).unwrap();
        assert_eq!(back, release);
    }
}
