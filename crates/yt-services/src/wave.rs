//! Radio via ytmusicapi get_watch_playlist(radio=true).

use std::sync::Arc;

use md3_model::{RadioSeedGroup, TrackItem};
use tokio::runtime::Handle;
use tokio::sync::watch;

use mf_platform::SettingsStore;
use yt_player::PlayerHandle;
use md3_model::PlayContext;

use crate::auth::AuthService;
use crate::history::HistoryService;
use crate::library::LibraryService;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WaveSnapshot {
    pub active: bool,
    pub loading: bool,
    pub station_title: String,
    pub station_uri: String,
    pub selected: String,
    pub tracks: Arc<Vec<TrackItem>>,
    pub seeds: Arc<Vec<RadioSeedGroup>>,
    pub locked: bool,
    pub error_text: String,
}

pub struct WaveService {
    rt: Handle,
    auth: Arc<AuthService>,
    player: PlayerHandle,
    _library: Arc<LibraryService>,
    _history: Arc<HistoryService>,
    _settings: Arc<SettingsStore>,
    snapshot: watch::Sender<WaveSnapshot>,
}

impl WaveService {
    pub fn new(
        rt: Handle,
        auth: Arc<AuthService>,
        player: PlayerHandle,
        library: Arc<LibraryService>,
        history: Arc<HistoryService>,
        settings: Arc<SettingsStore>,
    ) -> Arc<Self> {
        let (snapshot, _) = watch::channel(WaveSnapshot { locked: false, ..Default::default() });
        Arc::new(Self { rt, auth, player, _library: library, _history: history, _settings: settings, snapshot })
    }

    pub fn play(&self) {
        let selected = self.snapshot.borrow().selected.clone();
        if selected.is_empty() {
            let _ = self.snapshot.send(WaveSnapshot { error_text: "Play something first — the radio will start from your last track".into(), ..self.snapshot.borrow().clone() });
            return;
        }
        self.play_station(selected, String::new());
    }

    pub fn play_station(&self, seed: String, title: String) {
        let api = self.auth.api();
        let rt = self.rt.clone();
        let snapshot = self.snapshot.clone();
        let player = self.player.clone();
        let video_id = seed.rsplit(':').next().unwrap_or(&seed).to_string();
        let title = if title.is_empty() { format!("Radio {seed}") } else { title };
        rt.spawn(async move {
            let _ = snapshot.send(WaveSnapshot { loading: true, station_title: title.clone(), ..snapshot.borrow().clone() });
            match api.get_watch_playlist(Some(&video_id), None, true, 25).await {
                Ok(v) => {
                    let list: Vec<serde_json::Value> = v.as_array().cloned().unwrap_or_default();
                    let tracks: Vec<TrackItem> = list.iter().map(yt_api::map::track_item).collect();
                    if tracks.is_empty() {
                        let _ = snapshot.send(WaveSnapshot { loading: false, error_text: "Radio returned no tracks".into(), ..snapshot.borrow().clone() });
                        return;
                    }
                    let ctx = PlayContext::with_uri("radio", format!("ytmusic:radio:{video_id}")).titled(title.clone());
                    let _ = snapshot.send(WaveSnapshot {
                        active: true, loading: false,
                        station_uri: format!("ytmusic:radio:{video_id}"),
                        station_title: title,
                        selected: seed,
                        tracks: Arc::new(tracks.clone()),
                        error_text: String::new(),
                        ..snapshot.borrow().clone()
                    });
                    player.play_items(tracks, 0, ctx);
                }
                Err(e) => {
                    let _ = snapshot.send(WaveSnapshot { loading: false, error_text: e.user_text(), ..snapshot.borrow().clone() });
                }
            }
        });
    }

    pub fn select(&self, seed: String) {
        let _ = self.snapshot.send(WaveSnapshot { selected: seed, ..self.snapshot.borrow().clone() });
    }
    pub fn open(&self, seed: String) { self.play_station(seed, String::new()); }
    pub fn refresh(&self) {
        let seed = self.snapshot.borrow().selected.clone();
        if !seed.is_empty() { self.play_station(seed, String::new()); }
    }
    pub fn save(&self) {}
    pub fn snapshot(&self) -> WaveSnapshot { self.snapshot.borrow().clone() }
    pub fn watch(&self) -> watch::Receiver<WaveSnapshot> { self.snapshot.subscribe() }
}
