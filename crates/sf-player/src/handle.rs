//! PlayerHandle — the player facade for services and bridges (the PlayerHandle shape from yamusic-md3, adjusted for Spotify).
//! Commands do not block: they go to the actor on tokio (mpsc), and the result arrives in the snapshot/events.
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use md3_model::{PlayContext, TrackItem};
use sf_api::ApiGateway;
use tokio::runtime::Handle;
use tokio::sync::{broadcast, mpsc, watch};

use crate::engine::{EngineConfig, Rootlist};
use crate::state::actor::{Actor, Command, Msg, Reconnect, Restart};
use crate::state::backend::{SpotifyBackend, WebBackend};
use crate::state::connect::{self, ConnectBackend};
use crate::state::core::Core;
use crate::state::host::EngineHost;
use crate::state::session::SessionData;
use crate::{
    Credentials, EngineStarted, LocalPlayback, PlayerConfig, PlayerError, PlayerEvent, PlayerSnapshot,
    PlayerSubscription, Repeat, Session,
};

/// Clone — cheap.
#[derive(Clone)]
pub struct PlayerHandle {
    inner: Arc<Inner>,
}

struct Inner {
    tx: mpsc::UnboundedSender<Msg>,
    snapshot: Arc<watch::Sender<PlayerSnapshot>>,
    events: broadcast::Sender<PlayerEvent>,
    host: Arc<EngineHost>,
    rt: Handle,
    session_file: Option<PathBuf>,
    connect: Arc<ConnectBackend>,
}

/// An English translation key; outward it is `mf_i18n::t(NOT_READY)`.
const NOT_READY: &str = "Playback on this computer is off";

impl PlayerHandle {
    /// Start the actor on `rt`. The Web API (polling /me/player, remote commands, the queue) — through `api`.
    pub fn start(rt: &Handle, config: PlayerConfig, api: Arc<ApiGateway>) -> Result<Self, PlayerError> {
        let backend = Arc::new(SpotifyBackend::new(api, config.http.clone(), config.cover_dir.clone()));
        Ok(Self::with_backend(rt, config, backend))
    }

    /// Startup with any Web API (tests substitute a fake one).
    pub(crate) fn with_backend(rt: &Handle, config: PlayerConfig, backend: Arc<dyn WebBackend>) -> Self {
        let initial = PlayerSnapshot { volume: config.volume, direction: 1, current_index: -1, ..Default::default() };
        let snapshot = Arc::new(watch::channel(initial).0);
        let (events, _) = broadcast::channel(1024);
        let (tx, rx) = mpsc::unbounded_channel();
        let host = Arc::new(EngineHost::new(tx.clone()));
        // Connect (state, queue, commands) — through the engine session; the Web API is the fallback path
        let connect = connect::wrap(backend, &host, &tx);
        let backend: Arc<dyn WebBackend> = connect.clone();
        let reconnect: Reconnect = {
            let host = host.clone();
            let rt = rt.clone();
            Arc::new(move |volume| host.reconnect(&rt, volume))
        };
        let restart: Restart = {
            let host = host.clone();
            let rt = rt.clone();
            Arc::new(move |config, volume, load| host.restart(&rt, Some(config), volume, load))
        };
        let core = Core::new(config.volume, config.session_file.is_some());
        let session_file = config.session_file.clone();
        {
            let snapshot = snapshot.clone();
            let events = events.clone();
            let tx = tx.clone();
            let session_file = session_file.clone();
            rt.spawn(async move {
                Actor::new(core, rx, tx, snapshot, events, backend, reconnect, restart, session_file).run().await;
            });
        }
        let handle = Self { inner: Arc::new(Inner { tx, snapshot, events, host, rt: rt.clone(), session_file, connect }) };
        handle.load_session();
        handle
    }

    pub(crate) fn connect(&self) -> &Arc<ConnectBackend> {
        &self.inner.connect
    }

    fn send(&self, command: Command) {
        let _ = self.inner.tx.send(Msg::Command(command));
    }

    fn load_session(&self) {
        let tx = self.inner.tx.clone();
        let path = self.inner.session_file.clone();
        self.inner.rt.spawn_blocking(move || {
            let data = path.as_deref().and_then(SessionData::load);
            let _ = tx.send(Msg::Restore(data));
        });
    }

    pub fn snapshot(&self) -> PlayerSnapshot {
        self.inner.snapshot.borrow().clone()
    }

    pub fn subscribe(&self) -> PlayerSubscription {
        PlayerSubscription { snapshot: self.inner.snapshot.subscribe(), events: self.inner.events.subscribe() }
    }

    // --- local engine (librespot) ---------------------------------------------------------

    /// Bring up the Engine (Session + Spirc) with credentials; called by AuthService after the Premium check.
    /// Ok — the device is ready; reusable — save it in the Playback slot.
    pub async fn connect_local(&self, credentials: Credentials, config: EngineConfig) -> Result<EngineStarted, String> {
        self.inner.host.connect(credentials, config, None).await
    }

    /// Restart with new settings (bitrate, normalization, gapless, output device, cache, name)
    /// carrying over the context, track, position, modes, and the manual queue.
    pub fn restart_engine(&self, config: EngineConfig) {
        self.send(Command::RestartEngine(Box::new(config)));
    }

    /// Stop the local engine (sign-out, "Disconnect playback").
    pub fn disconnect_local(&self) {
        self.inner.host.disconnect();
    }

    pub fn local_playback(&self) -> LocalPlayback {
        self.inner.snapshot.borrow().local.clone()
    }

    /// The librespot session (Clone, Arc inside) — for session_reads, stations, rootlist; None — the engine is not ready.
    pub fn session(&self) -> Option<Session> {
        self.inner.host.engine().map(|link| link.engine.session().clone())
    }

    /// The Connect device id of this PC (hex(sha1(device_name))), if the engine is ready.
    pub fn this_device_id(&self) -> Option<String> {
        self.inner.host.engine().map(|link| link.engine.device_id().to_owned())
    }

    /// Spotify lyrics (color-lyrics) through the session; Ok(None) — no lyrics.
    pub async fn lyrics_json(&self, track_uri: &str) -> Result<Option<serde_json::Value>, String> {
        let link = self.inner.host.engine().ok_or_else(|| mf_i18n::t(NOT_READY))?;
        link.engine.lyrics_json(track_uri).await.map_err(|error| format!("{error:#}"))
    }

    pub async fn rootlist(&self) -> Result<Rootlist, String> {
        let link = self.inner.host.engine().ok_or_else(|| mf_i18n::t(NOT_READY))?;
        link.engine.rootlist().await.map_err(|error| format!("{error:#}"))
    }

    // --- playback start ------------------------------------------------------------------------------

    /// A list from the QML model, starting at row: if context.context_uri is present — Load{context_uri, offset_uri};
    /// if not (search/filter/radio list) — Load{uris (≤500 from row), offset}. The manual queue is preserved.
    pub fn play_items(&self, items: Vec<TrackItem>, row: usize, context: PlayContext) {
        self.send(Command::PlayItems { items, row, context });
    }

    /// "Shuffle": shuffle on and start from a random available track.
    pub fn shuffle_from(&self, items: Vec<TrackItem>, context: PlayContext) {
        self.send(Command::ShuffleFrom { items, context });
    }

    /// A context by URI (playlist/album/artist/show/station), from the offset_uri track or from the beginning.
    pub fn play_context(&self, context: PlayContext, offset_uri: Option<String>, shuffle: Option<bool>) {
        self.send(Command::PlayContext { context, offset: offset_uri, shuffle });
    }

    /// A list of URIs from an index (radio "plays the shown songs", history).
    pub fn play_uris(&self, uris: Vec<String>, index: usize, context: PlayContext) {
        self.send(Command::PlayUris { uris, index, context });
    }

    /// Remember a row: the selected track is shown immediately, even when the list only knows its URI.
    pub fn remember(&self, track: TrackItem) {
        self.send(Command::Remember(Box::new(track)));
    }

    /// A podcast episode from a position (resume_ms).
    pub fn play_episode(&self, uri: String, position_ms: i64) {
        self.send(Command::PlayEpisode { uri, position_ms });
    }

    // --- queue (queue.md) -----------------------------------------------------------------

    /// Compatibility: a Player.queue row by index = play_queue_row(uid of that row).
    pub fn play_index(&self, index: usize) {
        self.send(Command::PlayIndex(index));
    }

    /// A click on a queue row = N×Next: the rows above are removed, the context continues (§5).
    pub fn play_queue_row(&self, uid: u64) {
        self.send(Command::PlayQueueRow(uid));
    }

    /// "Add to queue": after the manual rows, before the context; a double click < 1.5 s — one row (§2).
    pub fn add_to_queue(&self, items: Vec<TrackItem>) {
        self.send(Command::AddToQueue(items));
    }

    /// "Add to queue" by URI: a track/episode — one row; an album — all tracks in order, all or nothing.
    pub fn add_uri_to_queue(&self, uri: String) {
        self.send(Command::AddUriToQueue(uri));
    }

    /// Insert at the "Next in queue" position (local only; otherwise — at the end).
    pub fn insert_manual(&self, item: TrackItem, at: usize) {
        self.send(Command::InsertManual { item: Box::new(item), at });
    }

    /// Reordering within "Next in queue" (local only): `from`, `to` — indices within the section,
    /// `to` — the row's final place (like ListChange::Move: remove(from), insert(to)).
    pub fn move_manual(&self, from: usize, to: usize) {
        self.send(Command::MoveManual { from, to });
    }

    /// "Clear" — manual rows only (§7).
    pub fn clear_manual_queue(&self) {
        self.send(Command::ClearManualQueue);
    }

    /// Re-request the queue (/me/player/queue).
    pub fn refresh_queue(&self) {
        self.send(Command::RefreshQueue);
    }

    // --- transport ---------------------------------------------------------------------------

    pub fn toggle_play(&self) {
        self.send(Command::TogglePlay);
    }

    pub fn play(&self) {
        self.send(Command::Play);
    }

    pub fn pause(&self) {
        self.send(Command::Pause);
    }

    /// Spotify has no "stop" — pause.
    pub fn stop(&self) {
        self.send(Command::Stop);
    }

    /// Next immediately removes the top queue row (§4).
    pub fn next(&self) {
        self.send(Command::Next);
    }

    /// After 3 s — to the start of the track (done by librespot / the Web API), otherwise the previous one.
    pub fn previous(&self) {
        self.send(Command::Previous);
    }

    pub fn seek(&self, fraction: f64) {
        self.send(Command::Seek(fraction));
    }

    pub fn seek_ms(&self, ms: i64) {
        self.send(Command::SeekMs(ms));
    }

    /// ±ms from the current position (episodes: −10 s / +30 s).
    pub fn seek_relative(&self, delta_ms: i64) {
        self.send(Command::SeekRelative(delta_ms));
    }

    pub fn cycle_repeat(&self) {
        self.send(Command::CycleRepeat);
    }

    pub fn set_repeat(&self, repeat: Repeat) {
        self.send(Command::SetRepeat(repeat));
    }

    pub fn set_shuffle(&self, value: bool) {
        self.send(Command::SetShuffle(value));
    }

    /// 0..1; while dragging — VolumePreview, after ~150 ms of silence — Volume (remotely — PUT volume).
    pub fn set_volume(&self, value: f64) {
        self.send(Command::SetVolume(value.clamp(0.0, 1.0)));
    }

    pub fn set_levels_wanted(&self, value: bool) {
        self.send(Command::SetLevelsWanted(value));
    }

    /// Autoplay when the list ends (autoplay_seed) — immediately, without restarting the engine (calls glue).
    pub fn set_autoplay(&self, value: bool) {
        self.send(Command::SetAutoplay(value));
    }

    // --- Connect ---------------------------------------------------------------------------

    /// Move playback to this PC (librespot Transfer); the device disappeared — continue here
    /// from the last remote snapshot.
    pub fn transfer_here(&self, play: bool) {
        self.send(Command::TransferHere(play));
    }

    /// Move to a Connect device (PUT /me/player).
    pub fn transfer_to(&self, device_id: String, play: bool) {
        self.send(Command::TransferTo(device_id, play));
    }

    /// Poll /me/player now.
    pub fn refresh_remote(&self) {
        self.send(Command::RefreshRemote);
    }

    /// The window is visible — polling at the normal pace; hidden and nothing playing — less often.
    pub fn set_app_active(&self, active: bool) {
        self.send(Command::SetAppActive(active));
    }

    // --- lifecycle ---------------------------------------------------------------------

    /// Startup: pick up the account's current playback and the saved manual queue.
    pub fn restore_session(&self) {
        self.load_session();
    }

    /// Sign-out: forget the queue, the remote state, and this account's resume point.
    pub fn reset_account(&self) {
        self.send(Command::ResetAccount);
    }

    /// Sign-in will bring up the engine soon (true) or changed its mind (false): "Play" before connecting waits for the engine,
    /// instead of asking to choose a device (like playback_authorized in spotifast).
    pub fn expect_local(&self, expected: bool) {
        self.send(Command::ExpectLocal(expected));
    }

    /// Exit: save the session, stop the engine. Blocks for ≤ 2 s.
    pub fn shutdown(&self) {
        let (done, wait) = std::sync::mpsc::channel();
        if self.inner.tx.send(Msg::Shutdown(done)).is_ok() {
            let _ = wait.recv_timeout(Duration::from_millis(1500));
        }
        self.inner.host.shutdown();
    }
}
