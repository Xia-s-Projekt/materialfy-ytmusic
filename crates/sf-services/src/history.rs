//! Play history (provider "services-core"): local (librespot playback of Spotify does not
//! record; counted after 30 s or half the track, ≤ 500 records, mf_platform::dirs::history_file()) +
//! Spotify's recent, merging (data::journal::merged). Port of spotifast/src/history.rs.
//! Spotify's recent — through the librespot session (spclient recently-played + GraphQL labels, data::home_feed);
//! /me/player/recently-played of Web API — only without a session or when it fails, with a timeout.
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};

use md3_model::{PlayContext, TrackItem, contexts};
use sf_api::Operation;
use sf_player::{PlayerEvent, PlayerHandle};
use tokio::runtime::Handle;
use tokio::sync::broadcast;

use crate::auth::{AuthEvent, AuthService};
use crate::data::journal::{self, Journal};
use crate::data::{home_feed, known, route};
use crate::extra::feed;
use crate::lock;

/// How many Spotify records to keep in memory.
const REMOTE_KEPT: usize = 200;
/// Recent contexts from the session (as many as the web player has).
const SESSION_RECENT: u32 = 50;
/// Reading the recent through the session and the fallback Web API.
const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// One play.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PlayRecord {
    pub track: TrackItem,
    pub context: PlayContext,
    /// ISO time
    pub played_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HistoryEvent {
    Changed,
}

#[derive(Default)]
struct State {
    journal: Journal,
    remote: Vec<PlayRecord>,
    /// a late Web API answer after logout is discarded
    generation: u64,
    /// a read in flight: repeated refreshes (login, home) do not duplicate it
    loading: bool,
    /// the last read succeeded through the session (otherwise, when it comes up, read once more)
    via_session: bool,
    /// the session came up during a read without it — read once more when it finishes
    session_pending: bool,
}

pub struct HistoryService {
    rt: Handle,
    auth: Arc<AuthService>,
    player: PlayerHandle,
    events: broadcast::Sender<HistoryEvent>,
    path: PathBuf,
    state: Mutex<State>,
    me: Weak<HistoryService>,
}

/// Web API context type (Context.type) → PlayContext.kind.
fn context_kind(kind: &str) -> &str {
    match kind {
        "collection" => contexts::LIKED,
        "playlist" => contexts::PLAYLIST,
        "album" => contexts::ALBUM,
        "artist" => contexts::ARTIST,
        "show" => contexts::SHOW,
        other => other,
    }
}

impl HistoryService {
    /// Listens to PlayerEvent::TrackStarted/TrackEnded (only local = true is written locally).
    pub fn new(rt: Handle, auth: Arc<AuthService>, player: PlayerHandle) -> Arc<Self> {
        Self::with_file(rt, auth, player, mf_platform::dirs::history_file())
    }

    /// The same with a different history file (tests).
    pub fn with_file(rt: Handle, auth: Arc<AuthService>, player: PlayerHandle, path: PathBuf) -> Arc<Self> {
        let journal = Journal::load(&path);
        let service = Arc::new_cyclic(|me| Self {
            rt,
            auth,
            player,
            events: broadcast::channel(64).0,
            path,
            state: Mutex::new(State { journal, ..Default::default() }),
            me: me.clone(),
        });
        service.spawn_listeners();
        service
    }

    fn spawn_listeners(&self) {
        let mut player = self.player.subscribe().events;
        let me = self.me.clone();
        self.rt.spawn(async move {
            loop {
                match player.recv().await {
                    Ok(PlayerEvent::TrackEnded(ended)) => {
                        let Some(service) = me.upgrade() else { return };
                        if ended.local {
                            service.note_played(ended.track, ended.context, ended.played);
                        }
                    }
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        });
        let mut auth = self.auth.subscribe();
        let me = self.me.clone();
        self.rt.spawn(async move {
            loop {
                match auth.recv().await {
                    Ok(AuthEvent::SignedIn(_)) => {
                        let Some(service) = me.upgrade() else { return };
                        service.refresh();
                    }
                    Ok(AuthEvent::PlaybackReady(_)) => {
                        let Some(service) = me.upgrade() else { return };
                        service.session_ready();
                    }
                    Ok(AuthEvent::SignedOut) => {
                        let Some(service) = me.upgrade() else { return };
                        {
                            let mut state = lock(&service.state);
                            state.generation += 1;
                            state.remote.clear();
                            state.loading = false;
                            state.via_session = false;
                            state.session_pending = false;
                        }
                        let _ = service.events.send(HistoryEvent::Changed);
                    }
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        });
    }

    /// A track played out/skipped on this PC: count it if it was listened to enough (`played` — seconds).
    pub fn note_played(&self, track: TrackItem, context: PlayContext, played: f64) {
        if track.uri.is_empty() && track.track_id.is_empty() {
            return;
        }
        if played < journal::counts_after(track.duration_ms).as_secs_f64() {
            return;
        }
        let started = jiff::Timestamp::now()
            .checked_sub(jiff::SignedDuration::from_secs_f64(played.max(0.0)))
            .unwrap_or_else(|_| jiff::Timestamp::now());
        let record = PlayRecord { track, context, played_at: started.to_string() };
        lock(&self.state).journal.record(record);
        self.save_later();
        let _ = self.events.send(HistoryEvent::Changed);
    }

    fn save_later(&self) {
        let me = self.me.clone();
        self.rt.spawn_blocking(move || {
            if let Some(service) = me.upgrade() {
                service.flush();
            }
        });
    }

    /// The last plays (local + Web API), newest first.
    pub fn recent(&self, limit: usize) -> Vec<PlayRecord> {
        let state = lock(&self.state);
        let mut plays = journal::merged(state.journal.plays(), &state.remote);
        plays.truncate(limit);
        plays
    }

    /// Recent unique contexts (for radio seeds and "Recently played").
    pub fn recent_contexts(&self, limit: usize) -> Vec<PlayContext> {
        let mut seen = HashSet::new();
        self.recent(usize::MAX)
            .into_iter()
            .filter_map(|play| {
                let uri = play.context.context_uri.clone().filter(|uri| !uri.is_empty())?;
                seen.insert(uri).then_some(play.context)
            })
            .take(limit)
            .collect()
    }

    /// Time of the last play by context URI (sorting "Recent" and LibraryEntry.played_at).
    pub fn context_times(&self) -> HashMap<String, String> {
        let mut times = HashMap::new();
        for play in self.recent(usize::MAX) {
            if let Some(uri) = play.context.context_uri.filter(|uri| !uri.is_empty()) {
                times.entry(uri).or_insert(play.played_at);
            }
        }
        times
    }

    /// The session came up: repeat the read that went without it through it; one in progress — when it finishes.
    fn session_ready(&self) {
        let again = {
            let mut state = lock(&self.state);
            if state.via_session {
                false
            } else if state.loading {
                state.session_pending = true;
                false
            } else {
                true
            }
        };
        if again {
            self.refresh();
        }
    }

    /// Spotify's recent (up to 50): through the session, without it — Web API in the background. A read in progress is not duplicated.
    pub fn refresh(&self) {
        let generation = {
            let mut state = lock(&self.state);
            if state.loading {
                return;
            }
            state.loading = true;
            state.session_pending = false;
            state.generation
        };
        let me = self.me.clone();
        self.rt.spawn(async move {
            let Some(service) = me.upgrade() else { return };
            let via_session = sf_api::limits::background(service.read_recent(generation)).await;
            let again = {
                let mut state = lock(&service.state);
                if state.generation != generation {
                    return;
                }
                state.loading = false;
                if let Some(via_session) = via_session {
                    state.via_session = via_session;
                }
                std::mem::take(&mut state.session_pending) && !state.via_session
            };
            if again && route::account_session(&service.auth.api(), &service.player).is_some() {
                service.refresh();
            }
        });
    }

    /// One read of the recent; Some(whether through the session) — read.
    async fn read_recent(&self, generation: u64) -> Option<bool> {
        let api = self.auth.api();
        // the session answered (including "no history") — Web API is not asked
        let session = route::wait_account_session(&api, &self.player, route::STARTUP_SESSION_WAIT).await;
        let from_session = match session {
            Some(session) => {
                let http = api.http();
                let read = session_recent(&http, &session);
                match tokio::time::timeout(READ_TIMEOUT, read).await {
                    Ok(Ok(records)) => Some(records),
                    Ok(Err(error)) => {
                        tracing::debug!("recent via session: {error:#}; asking Web API");
                        None
                    }
                    Err(_) => None,
                }
            }
            None => None,
        };
        let via_session = from_session.is_some();
        let records = match from_session {
            Some(records) => records,
            None => {
                let read = async { api.client_for(Operation::UserData).await?.recently_played(50, None, None).await };
                let page = match tokio::time::timeout(READ_TIMEOUT, read).await {
                    Ok(Ok(page)) => page,
                    Ok(Err(error)) => {
                        self.auth.note_api_error(&error);
                        tracing::debug!("Spotify history failed to load: {error:#}");
                        return None;
                    }
                    Err(_) => {
                        tracing::debug!("Spotify history: Web API is taking too long");
                        return None;
                    }
                };
                known::remember_recordings(page.items.iter().map(|play| &play.track));
                let records: Vec<PlayRecord> = page
                    .items
                    .iter()
                    .map(|play| PlayRecord {
                        track: sf_api::map::track_item(&play.track),
                        context: play
                            .context
                            .as_ref()
                            .filter(|context| !context.uri.is_empty())
                            .map(|context| PlayContext::with_uri(context_kind(&context.kind), context.uri.clone()))
                            .unwrap_or_else(|| PlayContext::new(contexts::TRACKS)),
                        played_at: play.played_at.clone().unwrap_or_default(),
                    })
                    .collect();
                // the session's recent has incomplete tracks — only the Web API answer goes into the registry
                known::remember(records.iter().map(|record| &record.track));
                records
            }
        };
        let added = {
            let mut state = lock(&self.state);
            if state.generation != generation {
                return None;
            }
            let added = journal::absorb_remote(&mut state.remote, records);
            state.remote.truncate(REMOTE_KEPT);
            added
        };
        if added > 0 {
            let _ = self.events.send(HistoryEvent::Changed);
        }
        Some(via_session)
    }

    pub fn clear(&self) {
        {
            let mut state = lock(&self.state);
            state.journal.clear();
            state.remote.clear();
        }
        self.save_later();
        let _ = self.events.send(HistoryEvent::Changed);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<HistoryEvent> {
        self.events.subscribe()
    }

    /// Save to disk (exit).
    pub fn flush(&self) {
        lock(&self.state).journal.save(&self.path);
    }
}

/// The session account's recent contexts with labels and covers (GraphQL lookup; without labels — URI only).
async fn session_recent(http: &sf_api::Http, session: &sf_player::Session) -> anyhow::Result<Vec<PlayRecord>> {
    let recent = home_feed::recently_played(session, SESSION_RECENT).await?;
    if recent.is_empty() {
        return Ok(Vec::new());
    }
    let mut uris: Vec<String> = Vec::new();
    for context in &recent {
        for uri in [&context.uri, &context.last_track_uri] {
            if !uri.is_empty() && !uris.contains(uri) {
                uris.push(uri.clone());
            }
        }
    }
    let cards = home_feed::lookup(http, session, &uris).await.unwrap_or_else(|error| {
        tracing::debug!("recent labels: {error:#}");
        Vec::new()
    });
    Ok(feed::recent_records(&recent, &cards))
}
