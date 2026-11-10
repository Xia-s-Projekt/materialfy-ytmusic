//! PlayerHandle actor: queue + stream resolution + local audio output.
//! Simplified vs sf-player: local only, no Connect, no librespot session.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use md3_model::{DeviceItem, PlayContext, QueueRow, TrackItem};
use tokio::sync::{broadcast, mpsc, watch};

use yt_api::YtClient;

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
    Remember(TrackItem),
    /// Background metadata for a queue row that was created from a bare URI.
    EnrichRow { queue_gen: u64, index: usize, track_id: String, meta: StreamMeta },
    /// YouTube Music's own "up next" for a track started outside a list
    /// context (home shelf, search results, a single track).
    RadioRows { queue_gen: u64, tracks: Vec<TrackItem> },
    SetAutoplay(bool),
    /// "Listen here": playback is already local, so this just refreshes the
    /// device state the QML shows.
    TransferHere,
    /// Cover image downloaded for the current track; path is local file.
    CoverReady(String),
    Shutdown,
}

struct State {
    snapshot: PlayerSnapshot,
    queue: VecDeque<QueueRow>,
    manual: VecDeque<QueueRow>,
    /// «Далее из контекста»: радио, которое YouTube Music прислала на трек,
    /// запущенный не из списка (полка главной, поиск, одиночный трек).
    radio: VecDeque<QueueRow>,
    index: Option<usize>,
    uid: u64,
    autoplay: bool,
    playing: bool,
    position_ms: i64,
    duration_ms: i64,
    volume: f64,
    shuffle: bool,
    repeat: Repeat,
    context: PlayContext,
    load_seq: u64,
    /// Bumped every time the queue is replaced: results from an older
    /// enrichment pass are ignored.
    queue_gen: u64,
    /// Tracks the UI already knows by id (remembered before the queue is
    /// built from bare URIs). Rows for these ids get real titles immediately
    /// instead of showing the video id until the live lookup lands.
    remembered: std::collections::HashMap<String, TrackItem>,
    /// Rows already heard in this queue (by uid): shuffle picks among the rest.
    played: std::collections::HashSet<u64>,
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
        let cmd_tx2 = cmd_tx.clone();
        rt.spawn(async move {
            run(cmd_rx, cmd_tx2, snapshot_tx2, events_tx2, config_clone, client).await;
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
        self.send(Command::PlayContext(context, offset_uri, play));
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
    pub fn transfer_here(&self) {
        self.send(Command::TransferHere);
    }
    pub fn set_volume(&self, volume: f64) {
        self.send(Command::SetVolume(volume));
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
    pub fn shutdown(&self) {
        self.send(Command::Shutdown);
    }
}

async fn run(
    mut cmd: mpsc::UnboundedReceiver<Command>,
    cmd_tx: mpsc::UnboundedSender<Command>,
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
        radio: VecDeque::new(),
        index: None,
        uid: 0,
        autoplay: true,
        playing: false,
        position_ms: 0,
        duration_ms: 0,
        volume: config.volume,
        shuffle: false,
        repeat: Repeat::Off,
        context: PlayContext::default(),
        load_seq: 0,
        queue_gen: 0,
        remembered: std::collections::HashMap::new(),
        played: std::collections::HashSet::new(),
    };
    let audio = Audio::new(config.volume);
    let mut rng = Rng::new();
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
        let rows: Vec<QueueRow> = state
            .queue
            .iter()
            .chain(state.manual.iter())
            .chain(state.radio.iter())
            .cloned()
            .collect();
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
        snap.context_remaining = state.radio.len() as i32;
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
        let _ = snapshot_tx.send_replace(snap);
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
                        if let Some(next) = next_index(&mut state, &mut rng) {
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
                        // Включение перемешивания меняет только порядок
                        // воспроизведения: следующий трек выбирается из ещё не
                        // сыгранных строк (next_index).
                        state.shuffle = s;
                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::ModeChanged);
                    }
                    Command::SetRepeat(r) => {
                        state.repeat = r;
                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::ModeChanged);
                    }
                    Command::SetLevelsWanted(wanted) => {
                        state.snapshot.levels_wanted = wanted;
                        if !wanted {
                            state.snapshot.level = 0.0;
                        }
                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::LevelChanged);
                    }
                    Command::SetAutoplay(autoplay) => {
                        state.autoplay = autoplay;
                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::StateChanged);
                    }
                    Command::Remember(track) => {
                        if !track.track_id.is_empty() {
                            state.remembered.insert(track.track_id.clone(), track);
                        }
                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::QueueChanged);
                    }
                    Command::EnrichRow { queue_gen, index, track_id, meta } => {
                        // Stale: the queue was replaced since this pass started.
                        // Wrong row: the queue shifted under this index.
                        if queue_gen == state.queue_gen
                            && row_at(&state, index).is_some_and(|row| row.track.track_id == track_id)
                        {
                            let filled = row_at_mut(&mut state, index)
                                .is_some_and(|row| fill_row(&mut row.track, &meta));
                            if filled {
                                emit(&snapshot_tx, &events_tx, &state, PlayerEvent::QueueChanged);
                            }
                        }
                    }
                    Command::RadioRows { queue_gen, tracks } => {
                        // Устаревшая подборка: с момента запроса очередь заменили.
                        if queue_gen != state.queue_gen {
                            continue;
                        }
                        let queued: std::collections::HashSet<String> = state
                            .queue
                            .iter()
                            .chain(state.manual.iter())
                            .chain(state.radio.iter())
                            .map(|r| r.track.track_id.clone())
                            .collect();
                        let mut added = false;
                        for track in tracks {
                            if track.track_id.is_empty() || queued.contains(&track.track_id) {
                                continue;
                            }
                            state.uid += 1;
                            state.radio.push_back(QueueRow {
                                section: md3_model::QueueSection::Context,
                                uid: state.uid,
                                track,
                            });
                            added = true;
                        }
                        if added {
                            emit(&snapshot_tx, &events_tx, &state, PlayerEvent::QueueChanged);
                        }
                    }
                    Command::CoverReady(path) => {
                        state.snapshot.cover_file = path;
                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::CoverFileChanged);
                    }
                    Command::TransferHere => {
                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::DeviceChanged);
                    }
                    Command::PlayItems(tracks, row, context) => {
                        state.context = context;
                        if plays_as_list(&state.context) {
                            set_queue(&mut state, tracks, row);
                        } else {
                            // Полка главной, результаты поиска, история: список на
                            // экране — не очередь. Играет выбранный трек, а далее
                            // идёт радио, которое YouTube Music прислал на него.
                            let track = tracks.into_iter().nth(row).unwrap_or_default();
                            set_queue(&mut state, vec![track], 0);
                            spawn_radio(&state, &client, &cmd_tx);
                        }
                        spawn_enrichment(&state, &client, &cmd_tx);
                        state.position_ms = 0;
                        play_current(&mut state, &audio, &config, client.clone(), &load_tx);
                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::TrackChanged);
                    }
                    Command::ShuffleFrom(tracks, context) => {
                        state.context = context;
                        state.shuffle = true;
                        let start = rng.below(tracks.len().max(1));
                        set_queue(&mut state, tracks, start);
                        if plays_as_list(&state.context) {
                            spawn_enrichment(&state, &client, &cmd_tx);
                        } else {
                            spawn_radio(&state, &client, &cmd_tx);
                        }
                        state.position_ms = 0;
                        play_current(&mut state, &audio, &config, client.clone(), &load_tx);
                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::TrackChanged);
                    }
                    Command::PlayIndex(i) => {
                        if i < total_rows(&state) {
                            state.index = Some(i);
                            state.position_ms = 0;
                            play_current(&mut state, &audio, &config, client.clone(), &load_tx);
                            emit(&snapshot_tx, &events_tx, &state, PlayerEvent::TrackChanged);
                        }
                    }
                    Command::PlayQueueRow(uid) => {
                        let pos = state
                            .queue
                            .iter()
                            .chain(state.manual.iter())
                            .chain(state.radio.iter())
                            .position(|r| r.uid == uid);
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
                        if plays_as_list(&state.context) {
                            set_queue(&mut state, tracks, index);
                            spawn_enrichment(&state, &client, &cmd_tx);
                        } else {
                            // Список на экране (полка, поиск) — не очередь: играет
                            // выбранный трек, дальше — радио на него.
                            let track = tracks.into_iter().nth(index).unwrap_or_default();
                            set_queue(&mut state, vec![track], 0);
                            spawn_radio(&state, &client, &cmd_tx);
                        }
                        state.position_ms = 0;
                        play_current(&mut state, &audio, &config, client.clone(), &load_tx);
                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::TrackChanged);
                    }
                    Command::PlayContext(context, offset_uri, shuffle) => {
                        state.context = context.clone();
                        // «Перемешать» на контексте: порядок воспроизведения
                        // случайный (next_index), первый трек — тоже случайный.
                        // Обычный запуск контекста режим не трогает.
                        if let Some(s) = shuffle {
                            state.shuffle = s;
                        }
                        let mut first = |len: usize| if state.shuffle { rng.below(len.max(1)) } else { 0 };
                        if let Some(uri) = &context.context_uri {
                            let kind = yt_api::util::uri_kind(uri).unwrap_or("");
                            let id = uri.rsplit(':').next().unwrap_or("").to_string();
                            match kind {
                                "track" | "episode" => {
                                    let track = TrackItem { track_id: id.clone(), uri: uri.clone(), title: id, ..Default::default() };
                                    set_queue(&mut state, vec![track], 0);
                                    spawn_enrichment(&state, &client, &cmd_tx);
                                    spawn_radio(&state, &client, &cmd_tx);
                                    state.position_ms = 0;
                                    play_current(&mut state, &audio, &config, client.clone(), &load_tx);
                                }
                                "playlist" => {
                                    if let Ok(v) = client.get_playlist(&id, 100).await {
                                        let tracks = playlist_tracks(&v);
                                        if !tracks.is_empty() {
                                            let start = first(tracks.len());
                                            set_queue(&mut state, tracks, start);
                                            spawn_enrichment(&state, &client, &cmd_tx);
                                            state.position_ms = 0;
                                            play_current(&mut state, &audio, &config, client.clone(), &load_tx);
                                        }
                                    }
                                }
                                "album" => {
                                    if let Ok(v) = client.get_album(&id).await {
                                        let (_info, tracks) = yt_api::map::album_page(&v);
                                        if !tracks.is_empty() {
                                            let start = first(tracks.len());
                                            set_queue(&mut state, tracks, start);
                                            spawn_enrichment(&state, &client, &cmd_tx);
                                            state.position_ms = 0;
                                            play_current(&mut state, &audio, &config, client.clone(), &load_tx);
                                        }
                                    }
                                }
                                "artist" => {
                                    if let Ok(v) = client.get_watch_playlist(None, Some(&format!("RDBC{id}")), false, 25).await {
                                        let tracks = playlist_tracks(&v);
                                        if !tracks.is_empty() {
                                            let start = first(tracks.len());
                                            set_queue(&mut state, tracks, start);
                                            spawn_enrichment(&state, &client, &cmd_tx);
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
                                        spawn_enrichment(&state, &client, &cmd_tx);
                                        spawn_radio(&state, &client, &cmd_tx);
                                        state.position_ms = 0;
                                        play_current(&mut state, &audio, &config, client.clone(), &load_tx);
                                    }
                                }
                            }
                        } else if let Some(off) = &offset_uri {
                            let off_id = off.rsplit(':').next().unwrap_or("").to_string();
                            let track = TrackItem { track_id: off_id.clone(), uri: off.clone(), title: off_id, ..Default::default() };
                            set_queue(&mut state, vec![track], 0);
                            spawn_enrichment(&state, &client, &cmd_tx);
                            spawn_radio(&state, &client, &cmd_tx);
                            state.position_ms = 0;
                            play_current(&mut state, &audio, &config, client.clone(), &load_tx);
                        }
                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::TrackChanged);
                    }
                    Command::PlayEpisode(uri, _pos) => {
                        let id = uri.rsplit(':').next().unwrap_or(&uri).to_string();
                        let track = TrackItem { track_id: id.clone(), uri: uri.clone(), title: id, ..Default::default() };
                        set_queue(&mut state, vec![track], 0);
                        spawn_enrichment(&state, &client, &cmd_tx);
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
                        let bare = TrackItem { track_id: id.clone(), uri: uri.clone(), title: id, ..Default::default() };
                        let track = remember_or(&state.remembered, bare);
                        state.uid += 1;
                        state.manual.push_back(QueueRow { section: md3_model::QueueSection::Manual, uid: state.uid, track });
                        spawn_enrichment(&state, &client, &cmd_tx);
                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::QueueChanged);
                    }
                    Command::InsertManual(track, at) => {
                        let track = remember_or(&state.remembered, track);
                        state.uid += 1;
                        let at = at.min(state.manual.len());
                        state.manual.insert(at, QueueRow { section: md3_model::QueueSection::Manual, uid: state.uid, track });
                        spawn_enrichment(&state, &client, &cmd_tx);
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
                        Ok(loaded) => {
                            state.snapshot.codec = codec_label((loaded.bitrate / 1000) as u16);
                            match loaded.data {
                                LoadedData::File(bytes) => audio.load_bytes(bytes),
                                LoadedData::Stream(url) => audio.load(url),
                            }
                            state.snapshot.buffering = false;
                            state.playing = true;
                            // The player response knows the real title/artist/cover;
                            // a track started from a bare URI has only its id until now.
                            apply_meta(&mut state, loaded.meta);
                            // Download the cover so the theme engine can
                            // extract the accent colour from it.
                            let cover_url = row_at(&state, state.index.unwrap_or(0))
                                .map(|r| r.track.cover.clone())
                                .unwrap_or_default();
                            let track_id = row_at(&state, state.index.unwrap_or(0))
                                .map(|r| r.track.track_id.clone())
                                .unwrap_or_default();
                            if !cover_url.is_empty() && !track_id.is_empty() {
                                let cover_dir = config.cover_dir.clone();
                                let http = config.http.clone();
                                let cmd_tx = cmd_tx.clone();
                                tokio::spawn(async move {
                                    let safe: String = track_id
                                        .chars()
                                        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
                                        .collect();
                                    let out_path = cover_dir.join(format!("{safe}.jpg"));
                                    if !out_path.exists() {
                                        if let Ok(resp) = http.get(&cover_url).send().await {
                                            if let Ok(bytes) = resp.bytes().await {
                                                let _ = std::fs::create_dir_all(&cover_dir);
                                                let _ = std::fs::write(&out_path, &bytes);
                                            }
                                        }
                                    }
                                    if out_path.is_file() {
                                        let _ = cmd_tx.send(Command::CoverReady(out_path.to_string_lossy().to_string()));
                                    }
                                });
                            }
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
                        // A stream that dies mid-track also reaches EOF. Do
                        // not advance the queue as if the song had finished.
                        if state.duration_ms > 0 && state.position_ms + 5_000 < state.duration_ms {
                            state.snapshot.error_text =
                                "Playback stopped early — the stream was interrupted".to_string();
                            state.snapshot.buffering = false;
                            state.playing = false;
                            audio.stop();
                            emit(&snapshot_tx, &events_tx, &state, PlayerEvent::ErrorChanged);
                            continue;
                        }
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
                                let total = total_rows(&state);
                                if total > 0 && state.autoplay {
                                    // Очередь пройдена целиком: начинаем круг заново.
                                    let next = match next_index(&mut state, &mut rng) {
                                        Some(next) => next,
                                        None => {
                                            state.played.clear();
                                            if state.shuffle { rng.below(total) } else { 0 }
                                        }
                                    };
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
                            Repeat::Off => {
                                if let Some(next) = next_index(&mut state, &mut rng) {
                                    if state.autoplay {
                                        state.index = Some(next);
                                        state.position_ms = 0;
                                        play_current(&mut state, &audio, &config, client.clone(), &load_tx);
                                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::TrackChanged);
                                    } else {
                                        state.playing = false;
                                        audio.stop();
                                        emit(&snapshot_tx, &events_tx, &state, PlayerEvent::StateChanged);
                                    }
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
                        // Publish the audio level to the QML meters.
                        if state.snapshot.levels_wanted {
                            let level = audio.level() as f64;
                            if (level - state.snapshot.level).abs() > 0.005 {
                                state.snapshot.level = level;
                                emit(&snapshot_tx, &events_tx, &state, PlayerEvent::LevelChanged);
                            }
                        }
                    }
                }
            }
        }
    }
}

fn set_queue(state: &mut State, tracks: Vec<TrackItem>, start: usize) {
    state.queue.clear();
    state.radio.clear();
    state.played.clear();
    state.queue_gen += 1;
    for t in tracks {
        let t = remember_or(&state.remembered, t);
        state.uid += 1;
        state.queue.push_back(QueueRow { section: md3_model::QueueSection::Current, uid: state.uid, track: t });
    }
    state.index = if state.queue.is_empty() { None } else { Some(start.min(state.queue.len() - 1)) };
}

/// A remembered item (from the UI/catalogue) replaces a bare URI row so the
/// queue shows the real name immediately, not the video id.
fn remember_or(remembered: &std::collections::HashMap<String, TrackItem>, track: TrackItem) -> TrackItem {
    if row_is_bare(&track) {
        if let Some(known) = remembered.get(&track.track_id) {
            return known.clone();
        }
    }
    track
}

fn next_index(state: &mut State, rng: &mut Rng) -> Option<usize> {
    let total = total_rows(state);
    if total == 0 { return None; }
    let cur = state.index.unwrap_or(0);
    if !state.shuffle {
        let next = cur + 1;
        return (next < total).then_some(next);
    }
    // Перемешивание: следующий трек — любая ещё не сыгранная строка.
    let unheard: Vec<usize> = (0..total)
        .filter(|i| *i != cur && !state.played.contains(&uid_at(state, *i)))
        .collect();
    if unheard.is_empty() {
        return None;
    }
    Some(unheard[rng.below(unheard.len())])
}

/// Все строки очереди: контекст, свои, радио.
fn total_rows(state: &State) -> usize {
    state.queue.len() + state.manual.len() + state.radio.len()
}

/// uid строки по сквозному индексу (сначала строки контекста, затем свои).
fn uid_at(state: &State, index: usize) -> u64 {
    row_at(state, index).map(|r| r.uid).unwrap_or(u64::MAX)
}

/// Список, который играется сам по себе: альбом, плейлист, любимые треки,
/// топ исполнителя, подкаст. Всё остальное — полка главной, результаты
/// поиска, одиночный трек — играется радио: выбранный трек и то, что
/// YouTube Music рекомендует вслед за ним, а не список с экрана.
fn plays_as_list(context: &PlayContext) -> bool {
    matches!(context.kind.as_str(), "album" | "playlist" | "liked" | "artist" | "show" | "radio")
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

/// Fill the gaps of a row from player-response metadata. Only fills gaps:
/// a row that already came from a catalogue listing keeps its album and
/// artist strings. Returns whether anything changed.
fn fill_row(track: &mut TrackItem, meta: &StreamMeta) -> bool {
    let mut changed = false;
    // A track started from a bare URI carries its id as the title; the player
    // response has the real one, so it replaces the placeholder.
    if !meta.title.is_empty() && (track.title.is_empty() || track.title == track.track_id) {
        track.title = meta.title.clone();
        changed = true;
    }
    if !meta.artist.is_empty() && track.artists.is_empty() {
        track.artists = meta.artist.clone();
        changed = true;
    }
    if !meta.cover.is_empty() && track.cover.is_empty() {
        track.cover = meta.cover.clone();
        track.cover_large = meta.cover.clone();
        changed = true;
    }
    if meta.duration_ms > 0 && track.duration_ms == 0 {
        track.duration_ms = meta.duration_ms;
        changed = true;
    }
    changed
}

/// A row that came from a bare URI: the title is empty or just the video id.
fn row_is_bare(track: &TrackItem) -> bool {
    track.title.is_empty() || track.title == track.track_id
}

/// Fetch metadata for the rows that have none yet (bare-URI queue entries) in
/// the background, then hand each result back to the actor as a command.
/// Bounded: at most ENRICH_ROWS rows, ENRICH_PARALLEL fetches at once.
fn spawn_enrichment(state: &State, client: &Arc<YtClient>, cmd_tx: &mpsc::UnboundedSender<Command>) {
    let queue_gen = state.queue_gen;
    let mut bare: Vec<(usize, String)> = state
        .queue
        .iter()
        .chain(state.manual.iter())
        .chain(state.radio.iter())
        .enumerate()
        .filter_map(|(index, row)| {
            row_is_bare(&row.track).then(|| (index, row.track.track_id.clone()))
        })
        .take(ENRICH_ROWS)
        .collect();
    if bare.is_empty() {
        return;
    }
    let client = client.clone();
    let cmd_tx = cmd_tx.clone();
    let permits = Arc::new(tokio::sync::Semaphore::new(ENRICH_PARALLEL));
    tokio::spawn(async move {
        let mut tasks = Vec::with_capacity(bare.len());
        for (index, track_id) in std::mem::take(&mut bare) {
            let client = client.clone();
            let cmd_tx = cmd_tx.clone();
            let permits = permits.clone();
            tasks.push(tokio::spawn(async move {
                let Ok(_permit) = permits.acquire().await else { return };
                let Ok(value) = client.get_video_details(&track_id).await else { return };
                let text = |key: &str| value.get(key).and_then(|x| x.as_str()).unwrap_or("").to_string();
                let meta = StreamMeta {
                    title: text("title"),
                    artist: text("artist"),
                    cover: text("cover"),
                    duration_ms: value.get("durationMs").and_then(|x| x.as_i64()).unwrap_or(0),
                };
                if !meta.title.is_empty() {
                    let _ = cmd_tx.send(Command::EnrichRow { queue_gen, index, track_id, meta });
                }
            }));
        }
        for task in tasks {
            let _ = task.await;
        }
    });
}

/// Ask YouTube Music what should follow the track that just started and hand
/// the answer back as up-next rows. This is the radio the service recommends
/// for that track — not the list the track was clicked in.
fn spawn_radio(state: &State, client: &Arc<YtClient>, cmd_tx: &mpsc::UnboundedSender<Command>) {
    let Some(track) = row_at(state, state.index.unwrap_or(0)) else { return };
    let seed = track.track.track_id.clone();
    if seed.is_empty() {
        return;
    }
    let queue_gen = state.queue_gen;
    let client = client.clone();
    let cmd_tx = cmd_tx.clone();
    tokio::spawn(async move {
        let Ok(value) = client.get_watch_playlist(Some(&seed), None, true, RADIO_ROWS).await else { return };
        let tracks = playlist_tracks(&value);
        if !tracks.is_empty() {
            let _ = cmd_tx.send(Command::RadioRows { queue_gen, tracks });
        }
    });
}

/// Write the metadata the player response carried into the current queue row.
fn apply_meta(state: &mut State, meta: StreamMeta) {
    let Some(index) = state.index else { return };
    let Some(row) = row_at_mut(state, index) else { return };
    fill_row(&mut row.track, &meta);
    // The state duration must track the row even when the row was already
    // enriched (otherwise a bare-URI track plays with a 0 duration).
    if row.track.duration_ms > 0 {
        state.duration_ms = row.track.duration_ms;
        state.snapshot.duration_ms = state.duration_ms;
    }
}

/// Row at a combined queue index (context rows first, then manual rows, then radio).
fn row_at(state: &State, index: usize) -> Option<&QueueRow> {
    state
        .queue
        .get(index)
        .or_else(|| state.manual.get(index.saturating_sub(state.queue.len())))
        .or_else(|| {
            state
                .radio
                .get(index.saturating_sub(state.queue.len() + state.manual.len()))
        })
}

/// The same row, mutable (context rows first, then manual, then radio).
fn row_at_mut(state: &mut State, index: usize) -> Option<&mut QueueRow> {
    let context_len = state.queue.len();
    let manual_len = state.manual.len();
    if index < context_len {
        return state.queue.get_mut(index);
    }
    if index < context_len + manual_len {
        return state.manual.get_mut(index - context_len);
    }
    state.radio.get_mut(index - context_len - manual_len)
}

/// How many bare queue rows one enrichment pass resolves.
const ENRICH_ROWS: usize = 50;
/// How many metadata fetches may run at once.
const ENRICH_PARALLEL: usize = 6;
/// How many tracks the service's radio adds as up-next.
const RADIO_ROWS: u32 = 25;

/// Randomness for shuffle without a crate: xorshift64* seeded from the clock.
struct Rng(u64);

impl Rng {
    fn new() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        Self(seed | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// A random index below `n` (`n` = 0 counts as 0).
    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next_u64() % n as u64) as usize }
    }
}

/// A finished background load, tagged with the request it belongs to.
struct LoadDone {    seq: u64,
    result: Result<LoadedStream, String>,
}

/// The resolved stream plus the track metadata that came with it.
struct LoadedStream {
    /// A fully downloaded file when available, else the CDN stream URL.
    /// Direct streaming caps at 1 MiB per token (verified against the CDN),
    /// so the downloaded file is the only way to play a whole song.
    data: LoadedData,
    bitrate: u32,
    /// Title/artist/cover/duration as reported by the InnerTube player response.
    meta: StreamMeta,
}

enum LoadedData {
    File(Vec<u8>),
    Stream(String),
}

/// Metadata for the playing track, when the player response carried it.
#[derive(Clone, Default)]
struct StreamMeta {
    title: String,
    artist: String,
    cover: String,
    duration_ms: i64,
}

/// Start loading the current row: resolve the InnerTube stream URL and hand
/// it to the audio thread, which streams bytes via Range requests.
/// Network work stays off the actor loop.
fn play_current(
    state: &mut State,
    audio: &Audio,
    _config: &PlayerConfig,
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
    // Shuffle must not offer this row again while it is the one playing.
    if let Some(uid) = row_at(state, state.index.unwrap_or(0)).map(|r| r.uid) {
        state.played.insert(uid);
    }
    let seq = state.load_seq;
    let load_tx = load_tx.clone();
    tokio::spawn(async move {
        let result = load_track(&client, &track).await;
        let _ = load_tx.send(LoadDone { seq, result });
    });
}

async fn load_track(
    client: &YtClient,
    track: &TrackItem,
) -> Result<LoadedStream, String> {
    // Google Video caps plain `url` GETs at exactly 1 MiB per token (verified
    // against the CDN: bytes 0-1048575 succeed, anything after 403s), so direct
    // streaming can never play a full song. The vendored yt-dlp downloads the
    // whole file (it does the JS `n` deciphering); the stream URL is only the
    // fallback for when the download is unavailable.
    //
    // The stream metadata comes along for free with the URL resolve below.
    let stream = client.get_stream_url(&track.track_id).await.ok();
    let meta = stream
        .as_ref()
        .map(|v| {
            let text = |key: &str| v.get(key).and_then(|x| x.as_str()).unwrap_or("").to_string();
            StreamMeta {
                title: text("title"),
                artist: text("artist"),
                cover: text("cover"),
                duration_ms: v.get("durationMs").and_then(|x| x.as_i64()).unwrap_or(0),
            }
        })
        .unwrap_or_default();
    let bitrate = stream
        .as_ref()
        .and_then(|v| v.get("bitrate").and_then(|x| x.as_u64()))
        .unwrap_or(0) as u32;

    let safe: String = track
        .track_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    let out_path = std::env::temp_dir().join(format!("materialfy_{safe}.m4a"));
    if let Ok(path) = client.download_audio(&track.track_id, &out_path).await {
        if let Ok(bytes) = std::fs::read(&path) {
            let _ = std::fs::remove_file(&path);
            if !bytes.is_empty() {
                return Ok(LoadedStream { data: LoadedData::File(bytes), bitrate, meta });
            }
        }
    }

    let url = stream
        .and_then(|v| v.get("url").and_then(|x| x.as_str()).map(str::to_string))
        .filter(|u| !u.is_empty())
        .ok_or_else(|| "No playable audio stream".to_string())?;
    Ok(LoadedStream { data: LoadedData::Stream(url), bitrate, meta })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with(count: usize) -> State {
        let mut state = State {
            snapshot: PlayerSnapshot::default(),
            queue: VecDeque::new(),
            manual: VecDeque::new(),
            radio: VecDeque::new(),
            index: None,
            uid: 0,
            autoplay: true,
            playing: false,
            position_ms: 0,
            duration_ms: 0,
            volume: 1.0,
            shuffle: false,
            repeat: Repeat::Off,
            context: PlayContext::default(),
            load_seq: 0,
            queue_gen: 0,
            remembered: std::collections::HashMap::new(),
            played: std::collections::HashSet::new(),
        };
        let tracks: Vec<TrackItem> = (0..count)
            .map(|i| TrackItem { track_id: format!("v{i}"), uri: format!("ytmusic:track:v{i}"), ..Default::default() })
            .collect();
        set_queue(&mut state, tracks, 0);
        state
    }

    fn ids(state: &State) -> Vec<String> {
        (0..state.queue.len() + state.manual.len() + state.radio.len())
            .filter_map(|i| row_at(state, i).map(|r| r.track.track_id.clone()))
            .collect()
    }

    #[test]
    fn sequential_queue_advances_in_order() {
        let mut state = state_with(3);
        let mut rng = Rng::new();
        let mut seen = vec![state.index.unwrap()];
        while let Some(next) = next_index(&mut state, &mut rng) {
            state.index = Some(next);
            state.played.insert(uid_at(&state, next));
            seen.push(next);
        }
        assert_eq!(seen, vec![0, 1, 2], "без перемешивания очередь идёт по порядку");
    }

    #[test]
    fn shuffle_plays_every_row_once_before_stopping() {
        let mut state = state_with(12);
        state.shuffle = true;
        let mut rng = Rng::new();
        state.played.insert(uid_at(&state, state.index.unwrap()));
        let mut played = Vec::new();
        while let Some(next) = next_index(&mut state, &mut rng) {
            played.push(row_at(&state, next).unwrap().track.track_id.clone());
            state.played.insert(uid_at(&state, next));
            state.index = Some(next);
        }
        assert_eq!(played.len(), 11, "каждая неслышанная строка звучит один раз");
        let mut unique = played.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), 11, "повторов в перемешанной очереди нет");
        assert!(!played.contains(&"v0".to_string()), "играющая строка не предлагается снова");
    }

    #[test]
    fn set_queue_keeps_the_context_order() {
        // Порядок воспроизведения решает только признак перемешивания:
        // список в очереди лежит как есть.
        let state = state_with(4);
        assert_eq!(ids(&state), ["v0", "v1", "v2", "v3"]);
        assert!(!state.shuffle);
    }

    #[test]
    fn radio_rows_land_after_manual_rows() {
        let mut state = state_with(1);
        state.uid += 1;
        state.manual.push_back(QueueRow {
            section: md3_model::QueueSection::Manual,
            uid: state.uid,
            track: TrackItem { track_id: "queued".into(), ..Default::default() },
        });
        state.uid += 1;
        state.radio.push_back(QueueRow {
            section: md3_model::QueueSection::Context,
            uid: state.uid,
            track: TrackItem { track_id: "radio".into(), ..Default::default() },
        });
        assert_eq!(ids(&state), ["v0", "queued", "radio"]);
        assert_eq!(row_at_mut(&mut state, 2).map(|r| r.track.track_id.as_str()), Some("radio"));
    }

    #[test]
    fn a_seed_track_advances_into_its_radio() {
        // Очередь из одного трека + радио: после него играет радио, а не стоп.
        let mut state = state_with(1);
        state.uid += 1;
        state.radio.push_back(QueueRow {
            section: md3_model::QueueSection::Context,
            uid: state.uid,
            track: TrackItem { track_id: "radio".into(), ..Default::default() },
        });
        let mut rng = Rng::new();
        state.played.insert(uid_at(&state, 0));
        assert_eq!(next_index(&mut state, &mut rng), Some(1), "далее — строка радио");
        assert_eq!(row_at(&state, 1).map(|r| r.track.track_id.as_str()), Some("radio"));
    }

    #[test]
    fn a_seed_track_with_shuffle_advances_into_its_radio() {
        let mut state = state_with(1);
        for id in ["r1", "r2", "r3"] {
            state.uid += 1;
            state.radio.push_back(QueueRow {
                section: md3_model::QueueSection::Context,
                uid: state.uid,
                track: TrackItem { track_id: id.into(), ..Default::default() },
            });
        }
        state.shuffle = true;
        let mut rng = Rng::new();
        state.played.insert(uid_at(&state, 0));
        let next = next_index(&mut state, &mut rng).expect("радио ещё не играло");
        assert!((1..=3).contains(&next), "далее — любая строка радио, получили {next}");
    }

    #[test]
    fn lists_play_in_order_everything_else_is_radio() {
        for kind in ["album", "playlist", "liked", "artist", "show", "radio"] {
            assert!(plays_as_list(&PlayContext::new(kind)), "{kind} играется списком");
        }
        for kind in ["", "tracks", "search", "track"] {
            assert!(!plays_as_list(&PlayContext::new(kind)), "{kind} играется радио");
        }
    }
}
