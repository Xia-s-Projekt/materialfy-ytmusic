//! PlayerHandle actor: queue + stream resolution + local audio output.
//! Simplified vs sf-player: local only, no Connect, no librespot session.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use md3_model::{DeviceItem, PlayContext, QueueRow, TrackItem};
use tokio::sync::{broadcast, mpsc, watch};

use yt_api::{YtClient, YtError};

use super::audio::Audio;
use super::{
    LocalPlayback, PlayerConfig, PlayerEvent, PlayerSnapshot, Repeat, Target, codec_label,
};

enum Command {
    PlayItems(Vec<TrackItem>, usize, PlayContext),
    PlayUris(Vec<String>, usize, PlayContext),
    PlayIndex(usize),
    TogglePlay,
    Play,
    Pause,
    Stop,
    Next,
    Previous,
    SeekMs(i64),
    SeekRelative(i64),
    PlayQueueRow(u64),
    PlayContext(PlayContext, Option<String>, Option<bool>),
    PlayEpisode(String, i64),
    ShuffleFrom(Vec<TrackItem>, PlayContext),
    AddToQueue(Vec<TrackItem>),
    AddUriToQueue(String),
    InsertManual(TrackItem, usize),
    MoveManual(usize, usize),
    ClearManualQueue,
    SetVolume(f64),
    SetShuffle(bool),
    SetRepeat(Repeat),
    SetLevelsWanted(bool),
    SetAppActive(bool),
    SaveVolume(u64),
    Remember(TrackItem),
    SetAutoplay(bool),
    TransferHere(bool),
    ExpectLocal(bool),
    ConnectLocal,
    DisconnectLocal,
    ResetAccount,
    RestartEngine,
    Resync,
    Shutdown,
}

struct State {
    snapshot: PlayerSnapshot,
    queue: VecDeque<QueueRow>,
    manual: VecDeque<QueueRow>,
    index: Option<usize>,
    uid: u64,
    autoplay: bool,
    app_active: bool,
    playing: bool,
    position_ms: i64,
    duration_ms: i64,
    volume: f64,
    shuffle: bool,
    repeat: Repeat,
    context: PlayContext,
    load_seq: u64,
}

pub struct PlayerHandle {
    snapshot_tx: watch::Sender<PlayerSnapshot>,
    events_tx: broadcast::Sender<PlayerEvent>,
    cmd: mpsc::UnboundedSender<Command>,
    config: PlayerConfig,
}

impl Clone for PlayerHandle {
    fn clone(&self) -> Self {
        Self {
            snapshot_tx: self.snapshot_tx.clone(),
            events_tx: self.events_tx.clone(),
            cmd: self.cmd.clone(),
            config: self.config.clone(),
        }
    }
}

impl PlayerHandle {
    pub fn start(
        rt: &tokio::runtime::Handle,
        config: PlayerConfig,
        client: Arc<YtClient>,
    ) -> anyhow::Result<Self> {
        let config_clone = config.clone();
        let (snapshot_tx, _) = watch::channel(PlayerSnapshot {
            volume: config.volume,
            local: LocalPlayback::Ready { device_id: "this-computer".into() },
            is_local: true,
            can_control: true,
            can_set_volume: true,
            target: Target::Local,
            ..Default::default()
        });
        let (events_tx, _) = broadcast::channel(256);
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let snapshot_tx2 = snapshot_tx.clone();
        let events_tx2 = events_tx.clone();
        rt.spawn(async move {
            run(cmd_rx, snapshot_tx2, events_tx2, config_clone, client).await;
        });
        Ok(Self { snapshot_tx, events_tx, cmd: cmd_tx, config })
    }

    pub fn snapshot(&self) -> PlayerSnapshot {
        self.snapshot_tx.borrow().clone()
    }

    pub fn watch(&self) -> watch::Receiver<PlayerSnapshot> {
        self.snapshot_tx.subscribe()
    }

    pub fn subscribe(&self) -> super::PlayerSubscription {
        super::PlayerSubscription {
            snapshot: self.snapshot_tx.subscribe(),
            events: self.events_tx.subscribe(),
        }
    }

    pub fn events(&self) -> broadcast::Receiver<PlayerEvent> {
        self.events_tx.subscribe()
    }

    pub fn local_playback(&self) -> LocalPlayback {
        self.snapshot().local
    }

    pub fn this_device_id(&self) -> Option<String> {
        Some("this-computer".to_string())
    }

    pub fn session(&self) -> Option<super::Session> {
        None
    }

    pub fn connect_changes(&self) -> broadcast::Receiver<PlayerEvent> {
        self.events_tx.subscribe()
    }

    pub async fn connect_devices(&self) -> Option<Vec<DeviceItem>> {
        Some(vec![DeviceItem {
            id: "this-computer".into(),
            name: "This computer".into(),
            kind: "computer".into(),
            is_active: true,
            is_this_computer: true,
            is_restricted: false,
            supports_volume: true,
            volume: (self.snapshot().volume * 100.0) as i32,
            source: "local".into(),
        }])
    }

    pub fn config_cover_dir(&self) -> &PathBuf {
        &self.config.cover_dir
    }

    // ---- commands (fire-and-forget, mirrors sf-player API) ----

    fn send(&self, cmd: Command) {
        let _ = self.cmd.send(cmd);
    }

    pub fn play_items(&self, items: Vec<TrackItem>, row: usize, context: PlayContext) {
        self.send(Command::PlayItems(items, row, context));
    }
    pub fn shuffle_from(&self, items: Vec<TrackItem>, context: PlayContext) {
        self.send(Command::ShuffleFrom(items, context));
    }
    pub fn play_index(&self, index: usize) {
        self.send(Command::PlayIndex(index));
    }
    pub fn toggle_play(&self) {
        self.send(Command::TogglePlay);
    }
    pub fn play(&self) {
        self.send(Command::Play);
    }
    pub fn pause(&self) {
        self.send(Command::Pause);
    }
    pub fn stop(&self) {
        self.send(Command::Stop);
    }
    pub fn next(&self) {
        self.send(Command::Next);
    }
    pub fn previous(&self) {
        self.send(Command::Previous);
    }
    pub fn seek_ms(&self, ms: i64) {
        self.send(Command::SeekMs(ms));
    }
    pub fn seek_relative(&self, delta: i64) {
        self.send(Command::SeekRelative(delta));
    }
    pub fn play_queue_row(&self, uid: u64) {
        self.send(Command::PlayQueueRow(uid));
    }
    pub fn play_context(
        &self,
        context: PlayContext,
        offset_uri: Option<String>,
        play: Option<bool>,
    ) {
        let _ = (offset_uri, play);
        self.send(Command::PlayContext(context, None, None));
    }
    pub fn play_episode(&self, uri: String, position_ms: i64) {
        self.send(Command::PlayEpisode(uri, position_ms));
    }
    pub fn play_uris(&self, uris: Vec<String>, index: usize, context: PlayContext) {
        self.send(Command::PlayUris(uris, index, context));
    }
    pub fn add_to_queue(&self, tracks: Vec<TrackItem>) {
        self.send(Command::AddToQueue(tracks));
    }
    pub fn add_uri_to_queue(&self, uri: String) {
        self.send(Command::AddUriToQueue(uri));
    }
    pub fn insert_manual(&self, track: TrackItem, at: usize) {
        self.send(Command::InsertManual(track, at));
    }
    pub fn move_manual(&self, from: usize, to: usize) {
        self.send(Command::MoveManual(from, to));
    }
    pub fn clear_manual_queue(&self) {
        self.send(Command::ClearManualQueue);
    }
    pub fn transfer_here(&self, play: bool) {
        self.send(Command::TransferHere(play));
    }
    pub fn set_app_active(&self, active: bool) {
        self.send(Command::SetAppActive(active));
    }
    pub fn set_volume(&self, volume: f64) {
        self.send(Command::SetVolume(volume));
    }
    pub fn save_volume(&self, generation: u64) {
        self.send(Command::SaveVolume(generation));
    }
    pub fn set_shuffle(&self, shuffle: bool) {
        self.send(Command::SetShuffle(shuffle));
    }
    pub fn set_repeat(&self, repeat: Repeat) {
        self.send(Command::SetRepeat(repeat));
    }
    pub fn set_levels_wanted(&self, wanted: bool) {
        self.send(Command::SetLevelsWanted(wanted));
    }
    pub fn remember(&self, track: TrackItem) {
        self.send(Command::Remember(track));
    }
    pub fn set_autoplay(&self, autoplay: bool) {
        self.send(Command::SetAutoplay(autoplay));
    }
    pub fn expect_local(&self, expect: bool) {
        self.send(Command::ExpectLocal(expect));
    }
    pub fn disconnect_local(&self) {
        self.send(Command::DisconnectLocal);
    }
    pub fn reset_account(&self) {
        self.send(Command::ResetAccount);
    }
    pub fn restart_engine(&self, _config: super::EngineConfig) {
        self.send(Command::RestartEngine);
    }
    pub fn resync(&self) {
        self.send(Command::Resync);
    }
    pub fn shutdown(&self) {
        self.send(Command::Shutdown);
    }
    pub fn transfer_to(&self, _device_id: &str, _play: bool) {}
    pub fn connect_volume(&self, _device_id: &str, _percent: u8) -> Option<()> {
        None
    }
}

fn build_queue(
    tracks: &[TrackItem],
    _start: Option<usize>,
    _context_section: md3_model::QueueSection,
    uid: &mut u64,
) -> Vec<QueueRow> {
    tracks
        .iter()
        .enumerate()
        .map(|(_i, t)| {
            *uid += 1;
            QueueRow {
                section: md3_model::QueueSection::Current,
                uid: *uid,
                track: t.clone(),
            }
        })
        .collect()
}

async fn run(
    mut cmd: mpsc::UnboundedReceiver<Command>,
    snapshot_tx: watch::Sender<PlayerSnapshot>,
    events_tx: broadcast::Sender<PlayerEvent>,
    config: PlayerConfig,
    client: Arc<YtClient>,
) {
    let mut state = State {
        snapshot: PlayerSnapshot {
            volume: config.volume,
            local: LocalPlayback::Ready { device_id: "this-computer".into() },
            is_local: true,
            can_control: true,
            can_set_volume: true,
            ..Default::default()
        },
        queue: VecDeque::new(),
        manual: VecDeque::new(),
        index: None,
        uid: 0,
        autoplay: true,
        app_active: true,
        playing: false,
        position_ms: 0,
        duration_ms: 0,
        volume: config.volume,
        shuffle: false,
        repeat: Repeat::Off,
        context: PlayContext::default(),
        load_seq: 0,
    };
    let audio = Audio::new(config.volume);
    let (load_tx, mut load_rx) = mpsc::unbounded_channel::<LoadDone>();
    // simple position ticker
    let (tick_tx, mut tick_rx) = mpsc::unbounded_channel::<()>();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(500));
        loop {
            interval.tick().await;
            if tick_tx.send(()).is_err() { break; }
        }
    });

    let emit = |snapshot_tx: &watch::Sender<PlayerSnapshot>, events_tx: &broadcast::Sender<PlayerEvent>, state: &State, event: PlayerEvent| {
        let rows: Vec<QueueRow> = state.queue.iter().chain(state.manual.iter()).cloned().collect();
        let mut snap = state.snapshot.clone();
        snap.queue = std::sync::Arc::new(rows);
        snap.playing = state.playing;
        snap.position_ms = state.position_ms;
        snap.duration_ms = state.duration_ms;
        snap.volume = state.volume;
        snap.shuffle = state.shuffle;
        snap.repeat = state.repeat;
        snap.context = state.context.clone();
        snap.track = row_at(state, state.index.unwrap_or(0)).map(|r| r.track.clone());
        snap.current_index = state.index.map(|i| i as i32).unwrap_or(-1);
        snap.manual_count = state.manual.len() as i32;
        snap.can_clear_queue = !state.manual.is_empty();
        snap.queue_reorderable = true;
        snap.is_local = true;
        snap.can_control = true;
        snap.can_set_volume = true;
        snap.target = Target::Local;
        snap.device = Some(DeviceItem {
            id: "this-computer".into(),
            name: "This computer".into(),
            kind: "computer".into(),
            is_active: true,
            is_this_computer: true,
            is_restricted: false,
            supports_volume: true,
            volume: (state.volume * 100.0) as i32,
            source: "local".into(),
        });
        let _ = snapshot_tx.send(snap);
        let _ = events_tx.send(event);
    };

    loop {
        tokio::select! {
            msg = cmd.recv() => {
                let Some(msg) = msg else { break };
                match msg {
                    Command::Shutdown => break,
                    Command::TogglePlay => {
                        state.playing = !state.playing;
                        audio.set_paused(!state.playing);
                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::StateChanged);
                    }
                    Command::Play => {
                        state.playing = true;
                        audio.set_paused(false);
                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::StateChanged);
                    }
                    Command::Pause => {
                        state.playing = false;
                        audio.set_paused(true);
                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::StateChanged);
                    }
                    Command::Stop => {
                        audio.stop();
                        state.playing = false;
                        state.position_ms = 0;
                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::StateChanged);
                    }
                    Command::Next => {
                        if let Some(next) = next_index(&state) {
                            state.index = Some(next);
                            state.position_ms = 0;
                            play_current(&mut state, &audio, &config, client.clone(), &load_tx);
                            emit(&snapshot_tx, &events_tx, &state, PlayerEvent::TrackChanged);
                        }
                    }
                    Command::Previous => {
                        if state.position_ms > 3000 {
                            state.position_ms = 0;
                            audio.seek(0);
                        } else if let Some(prev) = prev_index(&state) {
                            state.index = Some(prev);
                            state.position_ms = 0;
                            play_current(&mut state, &audio, &config, client.clone(), &load_tx);
                            emit(&snapshot_tx, &events_tx, &state, PlayerEvent::TrackChanged);
                        }
                    }
                    Command::SeekMs(ms) => {
                        state.position_ms = ms.max(0);
                        audio.seek(ms.max(0));
                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::PositionChanged);
                    }
                    Command::SeekRelative(delta) => {
                        let target = (state.position_ms + delta).max(0);
                        state.position_ms = target;
                        audio.seek(target);
                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::PositionChanged);
                    }
                    Command::SetVolume(v) => {
                        state.volume = v.clamp(0.0, 1.0);
                        audio.set_volume(state.volume);
                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::VolumeChanged);
                    }
                    Command::SetShuffle(s) => {
                        state.shuffle = s;
                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::ModeChanged);
                    }
                    Command::SetRepeat(r) => {
                        state.repeat = r;
                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::ModeChanged);
                    }
                    Command::SetLevelsWanted(_) | Command::SetAppActive(_) | Command::SaveVolume(_)
                    | Command::Remember(_) | Command::SetAutoplay(_) | Command::ExpectLocal(_)
                    | Command::DisconnectLocal | Command::ResetAccount | Command::RestartEngine
                    | Command::Resync => {
                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::StateChanged);
                    }
                    Command::ConnectLocal => {
                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::DeviceChanged);
                    }
                    Command::TransferHere(_) => {
                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::DeviceChanged);
                    }
                    Command::PlayItems(tracks, row, context) => {
                        state.context = context;
                        set_queue(&mut state, tracks, row);
                        state.position_ms = 0;
                        play_current(&mut state, &audio, &config, client.clone(), &load_tx);
                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::TrackChanged);
                    }
                    Command::ShuffleFrom(tracks, context) => {
                        state.context = context;
                        let mut tracks = tracks;
                        // Fisher-Yates with fastrand-like shuffle (no external crate)
                        for i in (1..tracks.len()).rev() {
                            let j = ((std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos() as usize).unwrap_or(0)) + i) % (i + 1);
                            tracks.swap(i, j);
                        }
                        set_queue(&mut state, tracks, 0);
                        state.position_ms = 0;
                        play_current(&mut state, &audio, &config, client.clone(), &load_tx);
                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::TrackChanged);
                    }
                    Command::PlayIndex(i) => {
                        if i < state.queue.len() + state.manual.len() {
                            state.index = Some(i);
                            state.position_ms = 0;
                            play_current(&mut state, &audio, &config, client.clone(), &load_tx);
                            emit(&snapshot_tx, &events_tx, &state, PlayerEvent::TrackChanged);
                        }
                    }
                    Command::PlayQueueRow(uid) => {
                        let pos = state.queue.iter().chain(state.manual.iter()).position(|r| r.uid == uid);
                        if let Some(p) = pos {
                            state.index = Some(p);
                            state.position_ms = 0;
                            play_current(&mut state, &audio, &config, client.clone(), &load_tx);
                            emit(&snapshot_tx, &events_tx, &state, PlayerEvent::TrackChanged);
                        }
                    }
                    Command::PlayUris(uris, index, context) => {
                        state.context = context;
                        let tracks: Vec<TrackItem> = uris.iter().map(|u| {
                            let id = u.rsplit(':').next().unwrap_or(u).to_string();
                            TrackItem { track_id: id.clone(), uri: u.clone(), title: id, ..Default::default() }
                        }).collect();
                        set_queue(&mut state, tracks, index);
                        state.position_ms = 0;
                        play_current(&mut state, &audio, &config, client.clone(), &load_tx);
                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::TrackChanged);
                    }
                    Command::PlayContext(context, offset_uri, _) => {
                        state.context = context.clone();
                        if let Some(uri) = &context.context_uri {
                            let kind = yt_api::util::uri_kind(uri).unwrap_or("");
                            let id = uri.rsplit(':').next().unwrap_or("").to_string();
                            match kind {
                                "track" | "episode" => {
                                    let track = TrackItem { track_id: id.clone(), uri: uri.clone(), title: id, ..Default::default() };
                                    set_queue(&mut state, vec![track], 0);
                                    state.position_ms = 0;
                                    play_current(&mut state, &audio, &config, client.clone(), &load_tx);
                                }
                                "playlist" => {
                                    if let Ok(v) = client.get_playlist(&id, 100).await {
                                        let tracks = playlist_tracks(&v);
                                        if !tracks.is_empty() {
                                            set_queue(&mut state, tracks, 0);
                                            state.position_ms = 0;
                                            play_current(&mut state, &audio, &config, client.clone(), &load_tx);
                                        }
                                    }
                                }
                                "album" => {
                                    if let Ok(v) = client.get_album(&id).await {
                                        let (_info, tracks) = yt_api::map::album_page(&v);
                                        if !tracks.is_empty() {
                                            set_queue(&mut state, tracks, 0);
                                            state.position_ms = 0;
                                            play_current(&mut state, &audio, &config, client.clone(), &load_tx);
                                        }
                                    }
                                }
                                "artist" => {
                                    if let Ok(v) = client.get_watch_playlist(None, Some(&format!("RDBC{id}")), false, 25).await {
                                        let tracks = playlist_tracks(&v);
                                        if !tracks.is_empty() {
                                            set_queue(&mut state, tracks, 0);
                                            state.position_ms = 0;
                                            play_current(&mut state, &audio, &config, client.clone(), &load_tx);
                                        }
                                    }
                                }
                                _ => {
                                    if let Some(off) = offset_uri {
                                        let off_id = off.rsplit(':').next().unwrap_or("").to_string();
                                        let track = TrackItem { track_id: off_id.clone(), uri: off.clone(), title: off_id, ..Default::default() };
                                        set_queue(&mut state, vec![track], 0);
                                        state.position_ms = 0;
                                        play_current(&mut state, &audio, &config, client.clone(), &load_tx);
                                    }
                                }
                            }
                        } else if let Some(off) = &offset_uri {
                            let off_id = off.rsplit(':').next().unwrap_or("").to_string();
                            let track = TrackItem { track_id: off_id.clone(), uri: off.clone(), title: off_id, ..Default::default() };
                            set_queue(&mut state, vec![track], 0);
                            state.position_ms = 0;
                            play_current(&mut state, &audio, &config, client.clone(), &load_tx);
                        }
                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::TrackChanged);
                    }
                    Command::PlayEpisode(uri, _pos) => {
                        let id = uri.rsplit(':').next().unwrap_or(&uri).to_string();
                        let track = TrackItem { track_id: id.clone(), uri: uri.clone(), title: id, ..Default::default() };
                        set_queue(&mut state, vec![track], 0);
                        state.position_ms = 0;
                        play_current(&mut state, &audio, &config, client.clone(), &load_tx);
                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::TrackChanged);
                    }
                    Command::AddToQueue(tracks) => {
                        for t in tracks {
                            state.uid += 1;
                            state.manual.push_back(QueueRow { section: md3_model::QueueSection::Manual, uid: state.uid, track: t });
                        }
                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::QueueChanged);
                    }
                    Command::AddUriToQueue(uri) => {
                        let id = uri.rsplit(':').next().unwrap_or(&uri).to_string();
                        state.uid += 1;
                        state.manual.push_back(QueueRow { section: md3_model::QueueSection::Manual, uid: state.uid, track: TrackItem { track_id: id.clone(), uri: uri.clone(), title: id, ..Default::default() } });
                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::QueueChanged);
                    }
                    Command::InsertManual(track, at) => {
                        state.uid += 1;
                        let at = at.min(state.manual.len());
                        state.manual.insert(at, QueueRow { section: md3_model::QueueSection::Manual, uid: state.uid, track });
                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::QueueChanged);
                    }
                    Command::MoveManual(from, to) => {
                        if from < state.manual.len() && to < state.manual.len() {
                            let row = state.manual.remove(from).unwrap();
                            state.manual.insert(to, row);
                        }
                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::QueueChanged);
                    }
                    Command::ClearManualQueue => {
                        state.manual.clear();
                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::QueueChanged);
                    }
                }
            }
            Some(done) = load_rx.recv() => {
                if done.seq == state.load_seq {
                    match done.result {
                        Ok((bytes, bitrate)) => {
                            state.snapshot.codec = codec_label((bitrate / 1000) as u16);
                            audio.load(bytes);
                            state.snapshot.buffering = false;
                            state.playing = true;
                            emit(&snapshot_tx, &events_tx, &state, PlayerEvent::StateChanged);
                        }
                        Err(message) => {
                            state.snapshot.error_text = message;
                            state.snapshot.buffering = false;
                            state.playing = false;
                            emit(&snapshot_tx, &events_tx, &state, PlayerEvent::ErrorChanged);
                        }
                    }
                }
            }
            _ = tick_rx.recv() => {
                if let Some(message) = audio.take_error() {
                    state.snapshot.error_text = message;
                    state.snapshot.buffering = false;
                    state.playing = false;
                    emit(&snapshot_tx, &events_tx, &state, PlayerEvent::ErrorChanged);
                } else if state.playing {
                    if audio.take_ended() {
                        state.position_ms = state.duration_ms;
                        let ended = row_at(&state, state.index.unwrap_or(0)).map(|r| r.track.clone());
                        if let Some(track) = ended {
                            let _ = events_tx.send(PlayerEvent::TrackEnded(super::TrackEnded {
                                track,
                                context: state.context.clone(),
                                played: state.position_ms as f64 / 1000.0,
                                end: state.duration_ms as f64 / 1000.0,
                                natural: true,
                                local: true,
                            }));
                        }
                        match state.repeat {
                            Repeat::One => {
                                state.position_ms = 0;
                                play_current(&mut state, &audio, &config, client.clone(), &load_tx);
                                emit(&snapshot_tx, &events_tx, &state, PlayerEvent::TrackChanged);
                            }
                            Repeat::All => {
                                let total = state.queue.len() + state.manual.len();
                                if total > 0 {
                                    let next = state.index.map(|i| i + 1).filter(|i| *i < total).unwrap_or(0);
                                    state.index = Some(next);
                                    state.position_ms = 0;
                                    play_current(&mut state, &audio, &config, client.clone(), &load_tx);
                                    emit(&snapshot_tx, &events_tx, &state, PlayerEvent::TrackChanged);
                                } else {
                                    state.playing = false;
                                }
                            }
                            Repeat::Off => {
                                if let Some(next) = next_index(&state) {
                                    state.index = Some(next);
                                    state.position_ms = 0;
                                    play_current(&mut state, &audio, &config, client.clone(), &load_tx);
                                    emit(&snapshot_tx, &events_tx, &state, PlayerEvent::TrackChanged);
                                } else {
                                    state.playing = false;
                                    audio.stop();
                                    emit(&snapshot_tx, &events_tx, &state, PlayerEvent::StateChanged);
                                }
                            }
                        }
                    } else {
                        let position = audio.position_ms();
                        if position != state.position_ms {
                            state.position_ms = position;
                            emit(&snapshot_tx, &events_tx, &state, PlayerEvent::PositionChanged);
                        }
                    }
                }
            }
        }
    }
}

fn set_queue(state: &mut State, tracks: Vec<TrackItem>, start: usize) {
    state.queue.clear();
    for t in tracks {
        state.uid += 1;
        state.queue.push_back(QueueRow { section: md3_model::QueueSection::Current, uid: state.uid, track: t });
    }
    state.index = if state.queue.is_empty() { None } else { Some(start.min(state.queue.len() - 1)) };
}

fn next_index(state: &State) -> Option<usize> {
    let total = state.queue.len() + state.manual.len();
    if total == 0 { return None; }
    let cur = state.index.unwrap_or(0);
    let next = cur + 1;
    if next < total { Some(next) } else { None }
}

fn prev_index(state: &State) -> Option<usize> {
    let cur = state.index?;
    if cur > 0 { Some(cur - 1) } else { None }
}

fn playlist_tracks(v: &serde_json::Value) -> Vec<TrackItem> {
    use yt_api::map;
    let mut out = Vec::new();
    if let Some(list) = v.get("tracks").and_then(|t| t.as_array()) {
        for item in list { out.push(map::track_item(item)); }
    } else if let Some(list) = v.get("contents").and_then(|t| t.as_array()) {
        for item in list { out.push(map::track_item(item)); }
    }
    out
}

/// Row at a combined queue index (context rows first, then manual rows).
fn row_at(state: &State, index: usize) -> Option<&QueueRow> {
    state
        .queue
        .get(index)
        .or_else(|| state.manual.get(index.saturating_sub(state.queue.len())))
}

/// A finished background load, tagged with the request it belongs to.
struct LoadDone {
    seq: u64,
    result: Result<(Vec<u8>, u32), String>,
}

/// Start loading the current row: resolve the stream, download it, and hand
/// the bytes to the audio thread. Network work stays off the actor loop.
fn play_current(
    state: &mut State,
    audio: &Audio,
    config: &PlayerConfig,
    client: Arc<YtClient>,
    load_tx: &mpsc::UnboundedSender<LoadDone>,
) {
    let Some(track) = row_at(state, state.index.unwrap_or(0)).map(|r| r.track.clone()) else {
        return;
    };
    state.duration_ms = track.duration_ms;
    state.position_ms = 0;
    state.snapshot.buffering = true;
    state.snapshot.error_text.clear();
    audio.stop();
    audio.set_paused(false);
    state.playing = true;
    state.load_seq += 1;
    let seq = state.load_seq;
    let http = config.http.clone();
    let load_tx = load_tx.clone();
    tokio::spawn(async move {
        let result = load_track(&client, &http, &track).await;
        let _ = load_tx.send(LoadDone { seq, result });
    });
}

async fn load_track(
    client: &YtClient,
    http: &reqwest::Client,
    track: &TrackItem,
) -> Result<(Vec<u8>, u32), String> {
    let song = client
        .get_song(&track.track_id)
        .await
        .map_err(|e| match e {
            YtError::NotSignedIn => "Sign in to YouTube Music to play".to_string(),
            other => other.user_text(),
        })?;
    let (url, bitrate) = super::stream::pick_audio_url(&song)
        .ok_or_else(|| "No playable audio stream (sign-in or age restriction?)".to_string())?;
    let response = http
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Stream download failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("Stream download failed: HTTP {}", response.status()));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Stream download failed: {e}"))?;
    Ok((bytes.to_vec(), bitrate))
}
