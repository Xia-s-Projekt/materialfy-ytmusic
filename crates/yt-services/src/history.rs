//! Local history (plays heard on this computer).

use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};

use md3_model::{PlayContext, TrackItem, contexts};
use tokio::runtime::Handle;
use tokio::sync::broadcast;

use yt_player::{PlayerEvent, PlayerHandle};

use crate::auth::AuthService;
use crate::lock;

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PlayRecord {
    pub track: TrackItem,
    pub context: PlayContext,
    pub played_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HistoryEvent {
    Changed,
}

#[derive(Default)]
struct State {
    journal: Vec<PlayRecord>,
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

impl HistoryService {
    pub fn new(rt: Handle, auth: Arc<AuthService>, player: PlayerHandle) -> Arc<Self> {
        Self::with_file(rt, auth, player, mf_platform::dirs::history_file())
    }
    pub fn with_file(rt: Handle, auth: Arc<AuthService>, player: PlayerHandle, path: PathBuf) -> Arc<Self> {
        let journal = std::fs::read_to_string(&path).ok()
            .and_then(|t| serde_json::from_str::<Vec<PlayRecord>>(&t).ok())
            .unwrap_or_default();
        let service = Arc::new_cyclic(|me| Self {
            rt: rt.clone(),
            auth: auth.clone(),
            player: player.clone(),
            events: broadcast::channel(64).0,
            path: path.clone(),
            state: Mutex::new(State { journal }),
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
                        if let Some(service) = me.upgrade() {
                            if ended.local { service.note_played(ended.track, ended.context, ended.played); }
                        }
                    }
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        });
        let mut auth = self.auth.subscribe();
        self.rt.spawn(async move {
            loop {
                match auth.recv().await {
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        });
    }

    fn note_played(&self, track: TrackItem, context: PlayContext, _played: f64) {
        if track.track_id.is_empty() { return; }
        let record = PlayRecord {
            track,
            context: if context.kind.is_empty() { PlayContext::new(contexts::TRACKS) } else { context },
            played_at: jiff::Timestamp::now().to_string(),
        };
        let mut state = lock(&self.state);
        state.journal.retain(|r| r.track.track_id != record.track.track_id);
        state.journal.push(record);
        if state.journal.len() > 500 {
            let excess = state.journal.len() - 500;
            state.journal.drain(..excess);
        }
        if let Some(parent) = self.path.parent() { let _ = std::fs::create_dir_all(parent); }
        if let Ok(json) = serde_json::to_string_pretty(&state.journal) {
            let _ = std::fs::write(&self.path, json);
        }
        drop(state);
        let _ = self.events.send(HistoryEvent::Changed);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<HistoryEvent> { self.events.subscribe() }
    pub fn recent(&self, limit: usize) -> Vec<PlayRecord> {
        let state = lock(&self.state);
        let mut out = state.journal.clone();
        out.reverse();
        out.truncate(limit);
        out
    }
    pub fn refresh(&self) { let _ = self.events.send(HistoryEvent::Changed); }
    pub fn session_ready(&self) {}
    pub fn flush(&self) {}
}
