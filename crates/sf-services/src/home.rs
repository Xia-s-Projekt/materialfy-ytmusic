//! Home (provider "services-extra"): shelves like spotifast/src/ui/home.rs + App::load_home —
//! "Quick access" (Liked + 5 playlists), "Recently played" (history: session/Web API + local),
//! "Made For You" and Spotify home's personal sections with their titles ("Recommended stations"…),
//! "New releases" (ReleasesService), "Continue podcasts", "Top artists", "Top tracks".
//! The home, "For You" and the top are read through the librespot session (data::home_feed: GraphQL and spclient) — without the quota
//! of Web API. Web API — only a fallback path without a session, with a timeout: 429 neither holds up nor drops the home. An empty
//! Spotify top (a new account) is replaced by the home's "Favorite artists" and a count by local history.
//! No more often than once in 10 min without force; visibility — SettingKey::HomeHidden. Answers of an old refresh are discarded.
use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use md3_model::{ArtistItem, EpisodeItem, HomeShelf, LibraryEntry, ShowItem, TrackItem, contexts};
use sf_api::Operation;
use sf_player::{PlayerHandle, Session};
use tokio::runtime::Handle;
use tokio::sync::{broadcast, watch};
use mf_platform::{SettingKey, SettingsStore};

use crate::ServiceError;
use crate::auth::{AuthEvent, AuthService};
use crate::data::{home_cache, home_feed, podcast_session, route};
use crate::extra::feed;
use crate::extra::shelves::{self, Play};
use crate::history::{HistoryEvent, HistoryService};
use crate::library::{LibraryEvent, LibraryService};
use crate::releases::ReleasesService;
use crate::lock;

/// No more often than once in 10 minutes without force.
pub const REFRESH_EVERY: Duration = Duration::from_secs(600);
const HISTORY_FOR_HOME: usize = 50;
/// How much history to count for the fallback top.
const HISTORY_FOR_TOP: usize = 500;
/// How long to wait for the session while the librespot engine connects.
const SESSION_WAIT: Duration = Duration::from_secs(20);
/// Reading through the session (home, "For You", top).
const SESSION_TIMEOUT: Duration = Duration::from_secs(25);
/// Fallback Web API: the shared key can answer 429 for hours — it cannot be waited for.
const WEB_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HomeSnapshot {
    pub loading: bool,
    pub shelves: Arc<Vec<HomeShelf>>,
    pub error_text: String,
}

/// Answers of the last refresh (session or Web API).
#[derive(Default)]
struct HomeData {
    loaded_at: Option<Instant>,
    /// the last refresh went through the session (otherwise, when it appears, refresh once more)
    via_session: bool,
    /// the session came up during a refresh without it — refresh once more when it finishes
    session_pending: bool,
    made_for_you: Vec<LibraryEntry>,
    /// Spotify home's personal sections — as shelves after "Made For You"
    feed: Vec<HomeShelf>,
    /// Spotify home's "Favorite artists" — the fallback top
    favorite_artists: Vec<ArtistItem>,
    podcasts: Vec<EpisodeItem>,
    top_artists: Vec<ArtistItem>,
    top_tracks: Vec<TrackItem>,
}

impl HomeData {
    /// Nothing from the network has arrived yet: the shelves from the cache still have room.
    fn is_empty(&self) -> bool {
        self.made_for_you.is_empty()
            && self.feed.is_empty()
            && self.favorite_artists.is_empty()
            && self.podcasts.is_empty()
            && self.top_artists.is_empty()
            && self.top_tracks.is_empty()
    }
}

pub struct HomeService {
    rt: Handle,
    auth: Arc<AuthService>,
    player: PlayerHandle,
    library: Arc<LibraryService>,
    history: Arc<HistoryService>,
    releases: Arc<ReleasesService>,
    settings: Arc<SettingsStore>,
    snapshot: watch::Sender<HomeSnapshot>,
    data: Mutex<HomeData>,
    generation: AtomicU64,
    me: Weak<HomeService>,
}

impl HomeService {
    pub fn new(
        rt: Handle,
        auth: Arc<AuthService>,
        player: PlayerHandle,
        library: Arc<LibraryService>,
        history: Arc<HistoryService>,
        releases: Arc<ReleasesService>,
        settings: Arc<SettingsStore>,
    ) -> Arc<Self> {
        let service = Arc::new_cyclic(|me| Self {
            rt: rt.clone(),
            auth,
            player,
            library,
            history,
            releases,
            settings,
            snapshot: watch::channel(HomeSnapshot::default()).0,
            data: Mutex::new(HomeData::default()),
            generation: AtomicU64::new(0),
            me: me.clone(),
        });
        rt.spawn(listen(Arc::downgrade(&service)));
        service
    }

    /// Refresh the shelves (force — without the 10-minute limit).
    pub fn refresh(&self, force: bool) {
        if !self.auth.is_signed_in() {
            self.snapshot.send_replace(HomeSnapshot {
                loading: false,
                shelves: Arc::new(Vec::new()),
                error_text: ServiceError::NotSignedIn.to_string(),
            });
            return;
        }
        {
            let mut data = lock(&self.data);
            let fresh = data.loaded_at.is_some_and(|at| at.elapsed() < REFRESH_EVERY);
            if fresh && !force {
                drop(data);
                self.publish();
                return;
            }
            data.loaded_at = Some(Instant::now());
            data.session_pending = false;
        }
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.snapshot.send_modify(|s| {
            s.loading = true;
            s.error_text.clear();
        });
        self.publish();
        self.history.refresh();
        self.hydrate();
        let Some(me) = self.me.upgrade() else { return };
        self.rt.spawn(async move { me.load(generation).await });
    }

    /// While the session is coming up (it can take tens of seconds), show the previous shelves from disk.
    /// Fresh ones will come next and replace them; if something is already loaded, the cache is not touched.
    fn hydrate(&self) {
        let Some(account) = self.auth.account_id() else { return };
        if !lock(&self.data).is_empty() {
            return;
        }
        let file = mf_platform::dirs::home_cache_file(account.as_str());
        let Some(me) = self.me.upgrade() else { return };
        self.rt.spawn(async move {
            let cached = tokio::task::spawn_blocking(move || home_cache::read(&file)).await.ok().flatten();
            let Some(cached) = cached else { return };
            me.adopt_cached(cached);
        });
    }

    /// Accept the cache: the same fields the network fills — the shelves are built by the usual publish.
    fn adopt_cached(&self, cached: home_cache::CachedHome) {
        {
            let mut data = lock(&self.data);
            // fresh answers came first — the cache is not needed
            if !data.is_empty() {
                return;
            }
            data.made_for_you = cached.made_for_you;
            data.feed = cached.feed;
            data.favorite_artists = cached.favorite_artists;
            data.podcasts = cached.podcasts;
            data.top_artists = cached.top_artists;
            data.top_tracks = cached.top_tracks;
        }
        self.publish();
    }

    /// The shelves — to disk: the next launch shows them without waiting for the session.
    fn save_cache(&self) {
        let Some(account) = self.auth.account_id() else { return };
        let cached = {
            let data = lock(&self.data);
            home_cache::CachedHome::new(
                data.made_for_you.clone(),
                data.feed.clone(),
                data.favorite_artists.clone(),
                data.podcasts.clone(),
                data.top_artists.clone(),
                data.top_tracks.clone(),
            )
        };
        if cached.is_empty() {
            return;
        }
        let file = mf_platform::dirs::home_cache_file(account.as_str());
        self.rt.spawn(async move {
            let _ = tokio::task::spawn_blocking(move || home_cache::write(&file, &cached)).await;
        });
    }

    pub fn snapshot(&self) -> HomeSnapshot {
        self.snapshot.borrow().clone()
    }

    pub fn watch(&self) -> watch::Receiver<HomeSnapshot> {
        self.snapshot.subscribe()
    }

    // --- internal ----------------------------------------------------------------------------

    fn current(&self, generation: u64) -> bool {
        self.generation.load(Ordering::SeqCst) == generation
    }

    /// Shelves by fresh data: Web API of the last refresh + library + history.
    fn publish(&self) {
        if !self.auth.is_signed_in() {
            return;
        }
        let hidden = shelves::hidden_keys(&self.settings.get_str(SettingKey::HomeHidden));
        let liked_uri = self.auth.account_id().map(|a| contexts::liked_uri(a.as_str())).unwrap_or_default();
        let playlists = self.library.playlists();
        let fresh = self.releases.snapshot();
        let built: Vec<HomeShelf> = {
            let data = lock(&self.data);
            // fallback top — by local history (counted only if Spotify did not give a top)
            let history_tracks = || -> Vec<TrackItem> {
                self.history.recent(HISTORY_FOR_TOP).into_iter().map(|r| r.track).collect()
            };
            let top = shelves::TOP_LIMIT as usize;
            shelves::ORDER
                .iter()
                .filter(|key| !hidden.contains(**key))
                .flat_map(|key| match *key {
                    shelves::QUICK => {
                        let liked = self.library.liked().len() as i64;
                        vec![shelves::shelf(key, &shelves::quick_tiles(&liked_uri, liked, &playlists))]
                    }
                    shelves::RECENT => vec![shelves::shelf(key, &shelves::recent_tiles(self.plays(), &playlists))],
                    // for "Made For You" — Spotify home's personal sections
                    shelves::MADE_FOR_YOU => std::iter::once(shelves::shelf(key, &data.made_for_you))
                        .chain(data.feed.iter().filter(|s| !hidden.contains(&s.key)).cloned())
                        .collect(),
                    shelves::NEW_RELEASES => {
                        vec![shelves::shelf(key, &shelves::new_release_tiles(fresh.radar.as_ref(), &fresh.releases))]
                    }
                    shelves::PODCASTS => vec![shelves::shelf(key, &data.podcasts)],
                    shelves::TOP_ARTISTS if !data.top_artists.is_empty() => vec![shelves::shelf(key, &data.top_artists)],
                    shelves::TOP_ARTISTS if !data.favorite_artists.is_empty() => {
                        vec![shelves::shelf(key, &data.favorite_artists)]
                    }
                    shelves::TOP_ARTISTS => {
                        let derived = feed::top_artists_from_plays(&history_tracks(), &data.favorite_artists, top);
                        vec![shelves::shelf(key, &derived)]
                    }
                    _ if !data.top_tracks.is_empty() => vec![shelves::shelf(key, &data.top_tracks)],
                    _ => vec![shelves::shelf(key, &feed::top_tracks_from_plays(&history_tracks(), top))],
                })
                .filter(|shelf| !shelf.items.is_empty())
                .collect()
        };
        self.snapshot.send_if_modified(|s| {
            if *s.shelves == built {
                return false;
            }
            s.shelves = Arc::new(built);
            true
        });
    }

    fn plays(&self) -> Vec<Play> {
        self.history
            .recent(HISTORY_FOR_HOME)
            .into_iter()
            .map(|r| Play {
                context: (!r.context.kind.is_empty() || r.context.context_uri.is_some()).then_some(r.context),
                track: r.track,
                played_at: r.played_at,
            })
            .collect()
    }

    fn store(&self, generation: u64, apply: impl FnOnce(&mut HomeData)) {
        if !self.current(generation) {
            return;
        }
        apply(&mut lock(&self.data));
        self.publish();
    }

    fn session(&self) -> Option<Session> {
        route::account_session(&self.auth.api(), &self.player)
    }

    /// The session, having waited for it up to SESSION_WAIT: after login the librespot engine does not come up at once.
    async fn wait_session(&self) -> Option<Session> {
        let started = Instant::now();
        loop {
            if let Some(session) = self.session() {
                return Some(session);
            }
            let coming = matches!(self.player.local_playback(), sf_player::LocalPlayback::Connecting);
            if !coming || started.elapsed() >= SESSION_WAIT {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// The session is ready: repeat the refresh that went without it (Web API) through it. A refresh in progress is not duplicated:
    /// it waits for the session itself, and if it still went without it — it will repeat when it finishes.
    fn session_ready(&self) {
        let loading = self.snapshot.borrow().loading;
        let again = {
            let mut data = lock(&self.data);
            let again = data.loaded_at.is_some() && !data.via_session;
            if again && loading {
                data.session_pending = true;
            }
            again && !loading
        };
        if again {
            self.refresh(true);
        }
    }

    async fn load(self: Arc<Self>, generation: u64) {
        let api = self.auth.api();
        let http = api.http();
        let session = self.wait_session().await;
        if !self.current(generation) {
            return;
        }
        lock(&self.data).via_session = session.is_some();
        let me = self.auth.account_id();

        // Spotify's home and "For You" through the session; without it — a search of Discover terms with the shared key
        let made_for_you = async {
            if let Some(session) = &session {
                let read = async {
                    tokio::join!(
                        home_feed::home(&http, session),
                        home_feed::made_for_you(&http, session),
                        home_feed::dj_cover(session)
                    )
                };
                let (home, browse, dj) = match tokio::time::timeout(SESSION_TIMEOUT, read).await {
                    Ok(answers) => answers,
                    Err(_) => (Err(anyhow::anyhow!("timeout")), Err(anyhow::anyhow!("timeout")), None),
                };
                if let Err(error) = &home {
                    tracing::debug!("Spotify home via session: {error:#}");
                }
                if let Err(error) = &browse {
                    tracing::debug!("“Made for you” via session: {error:#}");
                }
                if home.is_ok() || browse.is_ok() {
                    let home = home.unwrap_or_default();
                    let mut browse = browse.unwrap_or_default();
                    let publish = |browse: &[home_feed::FeedSection]| {
                        let tiles = feed::with_dj(feed::made_for_you_tiles(&home, browse), dj.as_deref());
                        let shelves = feed::feed_shelves(&home);
                        let favorites = feed::favorite_artists(&home);
                        self.store(generation, |d| {
                            d.made_for_you = tiles;
                            d.feed = shelves;
                            d.favorite_artists = favorites;
                        });
                    };
                    publish(&browse);
                    // day mixes come from browse as URIs alone — the labels are read in full through the session
                    if browse.iter().any(|s| s.cards.iter().any(|c| c.title.is_empty())) {
                        for section in &mut browse {
                            section.cards.truncate(feed::MADE_FOR_YOU_PER_SECTION * 2);
                            section.cards = home_feed::resolve_playlists(session, std::mem::take(&mut section.cards)).await;
                        }
                        publish(&browse);
                    }
                    return Ok(());
                }
            }
            let search = async {
                let client = api.client_for(Operation::PlaylistSearch).await?;
                let mut all = Vec::new();
                let mut seen = HashSet::new();
                for term in shelves::DISCOVER_TERMS {
                    match client.search(term, &["playlist"]).await {
                        Ok(results) => {
                            let found = results
                                .playlists
                                .map(|p| p.items)
                                .unwrap_or_default()
                                .iter()
                                .map(|p| sf_api::map::playlist_item(p, me.as_ref()))
                                .collect();
                            all.extend(
                                shelves::made_for_you(term, found).into_iter().filter(|p| seen.insert(p.playlist_id.clone())),
                            );
                        }
                        Err(e) => tracing::debug!("home: “{term}” not found: {e}"),
                    }
                }
                Ok::<_, sf_api::ApiError>(all)
            };
            let all = web(search).await?;
            let tiles: Vec<LibraryEntry> = all.iter().map(shelves::playlist_entry).collect();
            self.store(generation, |d| d.made_for_you = tiles);
            Ok::<_, String>(())
        };
        // top for 4 weeks: GraphQL through the session (empty is also an answer: history will replace it), without the session — Web API
        let top = async {
            if let Some(session) = &session {
                match tokio::time::timeout(SESSION_TIMEOUT, home_feed::top(&http, session, shelves::TOP_LIMIT)).await {
                    Ok(Ok((artists, tracks))) => {
                        let artists: Vec<ArtistItem> = artists.iter().map(sf_api::map::artist_item).collect();
                        let tracks: Vec<TrackItem> = tracks.iter().map(sf_api::map::track_item).collect();
                        self.store(generation, |d| {
                            d.top_artists = artists;
                            d.top_tracks = tracks;
                        });
                        return Ok(());
                    }
                    Ok(Err(error)) => tracing::debug!("top via session: {error:#}"),
                    Err(_) => tracing::debug!("top via session: timeout"),
                }
            }
            let read = async {
                let client = api.client_for(Operation::UserData).await?;
                let artists = client.top_artists("short_term", shelves::TOP_LIMIT).await?;
                let tracks = client.top_tracks("short_term", shelves::TOP_LIMIT, 0).await?;
                Ok::<_, sf_api::ApiError>((artists, tracks))
            };
            let (artists, tracks) = web(read).await?;
            let artists: Vec<ArtistItem> = artists.items.iter().map(sf_api::map::artist_item).collect();
            let tracks: Vec<TrackItem> = tracks.items.iter().map(sf_api::map::track_item).collect();
            self.store(generation, |d| {
                d.top_artists = artists;
                d.top_tracks = tracks;
            });
            Ok::<_, String>(())
        };
        let podcasts = async {
            // shows and their episodes — through the session (without the Web API quota); Web API — only if there is no session or it failed
            let read = async {
                let mut shows: Vec<ShowItem> = self.library.shows();
                if shows.is_empty() {
                    let saved = match session.as_ref() {
                        Some(session) => podcast_session::saved_shows(session)
                            .await
                            .inspect_err(|e| tracing::debug!("home: saved shows via session: {e:#}"))
                            .ok(),
                        None => None,
                    };
                    shows = match saved {
                        Some(saved) => saved.iter().map(|(_, show)| sf_api::map::show_item(show)).collect(),
                        None => {
                            let client = api.client_for(Operation::UserData).await?;
                            let page = client.saved_shows(0, 50).await?;
                            page.items.iter().map(|s| sf_api::map::show_item(&s.show)).collect()
                        }
                    };
                }
                shows.retain(|s| !s.show_id.is_empty());
                shows.truncate(shelves::HOME_PODCAST_SHOWS);
                let mut podcasts = Vec::new();
                let mut client = None;
                for show in shows {
                    let via_session = match session.as_ref() {
                        Some(session) => {
                            match podcast_session::show_page(&http, session, &show.show_id, 0, shelves::EPISODES_PER_SHOW).await {
                                Ok((_, page)) => Some(page),
                                Err(e) => {
                                    tracing::debug!("home: episodes of {} via session: {e:?}", show.show_id);
                                    None
                                }
                            }
                        }
                        None => None,
                    };
                    let page = match via_session {
                        Some(page) => Ok(page),
                        None => {
                            if client.is_none() {
                                client = Some(api.client_for(Operation::Catalog).await?);
                            }
                            let client = client.as_ref().expect("set above");
                            client.show_episodes(&show.show_id, 0, shelves::EPISODES_PER_SHOW).await
                        }
                    };
                    match page {
                        Ok(page) => {
                            let episodes = page.items.iter().map(|e| sf_api::map::episode_item(e, None)).collect();
                            podcasts.push((show, episodes));
                        }
                        // the shelf is optional: quota/429 — without this show
                        Err(e) => tracing::debug!("home: episodes of {}: {e}", show.show_id),
                    }
                }
                Ok::<_, sf_api::ApiError>(podcasts)
            };
            let podcasts = web(read).await?;
            let today = jiff::Zoned::now().date();
            let shelf = shelves::podcast_episodes(&podcasts, today);
            self.store(generation, |d| d.podcasts = shelf);
            Ok::<_, String>(())
        };
        let (a, b, c) = tokio::join!(made_for_you, top, podcasts);
        let errors: Vec<String> = [a, b, c].into_iter().filter_map(Result::err).collect();
        if !self.current(generation) {
            return;
        }
        let empty = self.snapshot.borrow().shelves.is_empty();
        // the shelves are a supplement: the error is visible only if nothing worked out
        let error = (errors.len() == 3 && empty).then(|| errors[0].clone());
        self.snapshot.send_modify(|s| {
            s.loading = false;
            s.error_text = error.unwrap_or_default();
        });
        // the shelves — to disk: the next launch will show them without waiting for the session
        self.save_cache();
        // the session came up while a refresh without it was going — refresh through it (without the Web API quota)
        let again = {
            let mut data = lock(&self.data);
            std::mem::take(&mut data.session_pending) && !data.via_session
        };
        if again && self.session().is_some() {
            self.refresh(true);
        }
    }

    fn clear(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        *lock(&self.data) = HomeData::default();
        self.snapshot.send_replace(HomeSnapshot::default());
    }
}

/// A fallback Web API request with a timeout; an error — as text for the user. In the background: it yields to pages and does not
/// go to the network while Web API is limited (the shelves are a supplement).
async fn web<T>(read: impl std::future::Future<Output = Result<T, sf_api::ApiError>>) -> Result<T, String> {
    match tokio::time::timeout(WEB_TIMEOUT, sf_api::limits::background(read)).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(ServiceError::from(error).to_string()),
        Err(_) => Err(mf_i18n::t("Spotify is taking too long to respond")),
    }
}

/// Login/logout, session readiness, library, history and shelf hiding → shelves.
async fn listen(weak: Weak<HomeService>) {
    let Some(me) = weak.upgrade() else { return };
    let mut auth = me.auth.subscribe();
    let mut library = me.library.subscribe();
    let mut history = me.history.subscribe();
    let mut settings = me.settings.subscribe();
    let mut releases = me.releases.watch();
    drop(me);
    loop {
        tokio::select! {
            event = auth.recv() => {
                let Some(me) = weak.upgrade() else { break };
                match event {
                    Ok(AuthEvent::SignedIn(_)) => me.refresh(true),
                    Ok(AuthEvent::SignedOut) => me.clear(),
                    Ok(AuthEvent::PlaybackReady(_)) => me.session_ready(),
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => me.publish(),
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            },
            event = library.recv() => match event {
                Ok(LibraryEvent::Playlists(_) | LibraryEvent::Liked(_) | LibraryEvent::Shows(_) | LibraryEvent::PinsChanged)
                | Err(broadcast::error::RecvError::Lagged(_)) => {
                    let Some(me) = weak.upgrade() else { break };
                    me.publish();
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            },
            event = history.recv() => match event {
                Ok(HistoryEvent::Changed) | Err(broadcast::error::RecvError::Lagged(_)) => {
                    let Some(me) = weak.upgrade() else { break };
                    me.publish();
                }
                Err(broadcast::error::RecvError::Closed) => break,
            },
            changed = releases.changed() => {
                if changed.is_err() {
                    break;
                }
                let Some(me) = weak.upgrade() else { break };
                me.publish();
            },
            key = settings.recv() => match key {
                Ok(SettingKey::HomeHidden) | Err(broadcast::error::RecvError::Lagged(_)) => {
                    let Some(me) = weak.upgrade() else { break };
                    me.publish();
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            },
        }
    }
}
