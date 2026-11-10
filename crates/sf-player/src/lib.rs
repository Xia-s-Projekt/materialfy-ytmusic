//! The Spotify player without Qt. Upper layer (provider "queue-state"): PlayerHandle in the yamusic-md3 shape — commands,
//! the PlayerSnapshot snapshot (watch) + PlayerEvent events (broadcast), merging local librespot and a remote
//! Connect device (Target), the queue by the rules of spotifast/docs/_reference/queue.md, optimistic
//! holds. The lower layer is the [`engine`] module (provider "engine").
//!
//! Threads: everything on the application's shared tokio runtime (Handle from the shell); librespot starts its own player thread,
//! fastframe-audio — the device thread. Outward — only watch/broadcast; the bridges wake Qt (CxxQtThread::queue).

pub mod engine;
pub mod handle;
/// JSON pushes of the dealer (social-connect) for sf-services
pub mod pushes;
/// internal "queue-state"
pub mod state;

use std::path::PathBuf;
use std::sync::Arc;

use md3_model::{DeviceItem, PlayContext, QueueRow, TrackItem};
use tokio::sync::{broadcast, watch};

pub use engine::sink::output_devices;
pub use engine::{EngineConfig, LoadSpec, LocalState, RepeatMode, Rootlist, RootlistEntry};
pub use handle::PlayerHandle;
pub use librespot_core::Session;
pub use librespot_core::spclient::SpClient;
pub use librespot_core::authentication::Credentials;

/// /me/player poll intervals (spotifast app.rs: REMOTE_POLL_*), s.
pub const REMOTE_POLL_ACTIVE_S: u64 = 4;
pub const REMOTE_POLL_IDLE_S: u64 = 20;
pub const REMOTE_POLL_UNSEEN_S: u64 = 30;
/// Optimistic state holds (AGENTS.md), ms.
pub const OPTIMISTIC_HOLD_MS: u64 = 2500;
pub const PLAYBACK_HOLD_MS: u64 = 6000;
/// How long the selected track stays on screen while the engine loads it (a cold start can take longer than 6 s).
pub const INTENT_CEILING_MS: u64 = 20_000;
pub const ASSUMED_CONTEXT_HOLD_MS: u64 = 8000;
pub const REMOTE_RECHECK_MS: u64 = 1200;
pub const QUEUE_RECHECK_MS: u64 = 700;
pub const QUEUE_STALE_RETRIES: u32 = 6;
pub const QUEUE_ADD_DEBOUNCE_MS: u64 = 1500;
pub const RESTART_BEFORE_PREVIOUS_MS: i64 = 3000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Repeat {
    #[default]
    Off,
    /// repeat the context (Spotify "context")
    All,
    /// repeat the track (Spotify "track")
    One,
}

impl Repeat {
    /// "off" | "all" | "one" (the value of Player.repeat in QML)
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::All => "all",
            Self::One => "one",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "off" => Some(Self::Off),
            "all" => Some(Self::All),
            "one" => Some(Self::One),
            _ => None,
        }
    }

    pub fn to_engine(self) -> RepeatMode {
        match self {
            Self::Off => RepeatMode::Off,
            Self::All => RepeatMode::Context,
            Self::One => RepeatMode::Track,
        }
    }

    pub fn from_engine(mode: RepeatMode) -> Self {
        match mode {
            RepeatMode::Off => Self::Off,
            RepeatMode::Context => Self::All,
            RepeatMode::Track => Self::One,
        }
    }

    /// off → all → one → off
    pub fn cycled(self) -> Self {
        Self::from_engine(self.to_engine().next())
    }
}

/// Where commands go: local librespot or a Connect device (Web API; None — active/unknown).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Target {
    #[default]
    Local,
    Remote(Option<String>),
}

/// State of local playback (librespot on this PC).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum LocalPlayback {
    /// the engine is not started (no permission/Premium or disabled)
    #[default]
    Unavailable,
    Connecting,
    Ready { device_id: String },
    Failed(String),
}

impl LocalPlayback {
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }
}

/// Result of connect_local: device id and reusable librespot credentials (save to the Playback slot).
pub struct EngineStarted {
    pub device_id: String,
    pub reusable: Option<Credentials>,
}

#[derive(Clone)]
pub struct PlayerConfig {
    /// mf_platform::dirs::cache_subdir("covers"): the current track's cover → cover_file
    pub cover_dir: PathBuf,
    /// the "Up next" queue and the resume point (queue.md item 9); None — do not save (snapshot mode)
    pub session_file: Option<PathBuf>,
    /// volume 0..1 from settings
    pub volume: f64,
    /// shared HTTP client (covers)
    pub http: sf_api::Http,
}

/// Snapshot of all Player properties for QML (the actor updates it before dispatching events).
#[derive(Clone, Debug, Default)]
pub struct PlayerSnapshot {
    /// current track/episode (None — none); hasTrack/trackId/title/artist/album/cover/coverLarge/uri/track
    pub track: Option<TrackItem>,
    /// "OGG 320" for local playback, "" for remote
    pub codec: String,
    /// path of the downloaded cover (theme from the cover, SMTC); "" — none
    pub cover_file: String,
    pub playing: bool,
    pub buffering: bool,
    /// The DJ is talking between tracks on this computer (Player.narrating).
    pub narrating: bool,
    /// with extrapolation between events/polls
    pub position_ms: i64,
    pub duration_ms: i64,
    /// 0..1
    pub volume: f64,
    pub shuffle: bool,
    pub repeat: Repeat,
    /// index of the section=current row in the queue, -1 — none
    pub current_index: i32,
    pub error_text: String,
    /// +1 — forward, -1 — back (track change animation)
    pub direction: i32,
    /// cover "breathing" 0..1 (local only and while levels_wanted)
    pub level: f64,
    pub levels_wanted: bool,
    pub context: PlayContext,
    /// Now + Up next + Next from context; changes in place — PlayerEvent::Queue
    pub queue: Arc<Vec<QueueRow>>,
    /// how many seconds of the current track have actually played (history)
    pub played_seconds: f64,

    // --- Spotify Connect ---
    pub target: Target,
    /// active device (None — nothing is playing anywhere)
    pub device: Option<DeviceItem>,
    /// this PC is playing (librespot)
    pub is_local: bool,
    pub local: LocalPlayback,
    /// whether control is allowed (!device.is_restricted)
    pub can_control: bool,
    pub can_set_volume: bool,
    /// a playback transfer is in progress (here or there)
    pub transferring: bool,
    /// the remote device disappeared (2 polls without it or 404)
    pub device_lost: bool,
    /// "N more tracks" in the context section
    pub context_remaining: i32,
    /// number of section=manual rows
    pub manual_count: i32,
    /// "Clear" is available (local only and there are manual rows)
    pub can_clear_queue: bool,
    /// dragging inside "Up next" (local only)
    pub queue_reorderable: bool,
}

impl PlayerSnapshot {
    pub fn has_track(&self) -> bool {
        self.track.is_some()
    }

    /// Player.source: context.type ("radio" | "liked" | "playlist" | …)
    pub fn source(&self) -> &str {
        &self.context.kind
    }

    pub fn position(&self) -> f64 {
        if self.duration_ms > 0 { self.position_ms as f64 / self.duration_ms as f64 } else { 0.0 }
    }

    pub fn is_episode(&self) -> bool {
        self.track.as_ref().is_some_and(|t| t.is_episode)
    }
}

/// A queue change — the bridge repeats it in the model so the list does not lose its scroll.
#[derive(Clone, Debug)]
pub enum QueueChange {
    Reset(Arc<Vec<QueueRow>>),
    Insert { at: usize, rows: Vec<QueueRow> },
    Remove { at: usize, count: usize },
    Move { from: usize, to: usize },
}

#[derive(Clone, Debug)]
pub struct TrackStarted {
    pub track: TrackItem,
    pub context: PlayContext,
    pub local: bool,
}

#[derive(Clone, Debug)]
pub struct TrackEnded {
    pub track: TrackItem,
    pub context: PlayContext,
    /// seconds
    pub played: f64,
    pub end: f64,
    /// played out by itself, not skipped
    pub natural: bool,
    pub local: bool,
}

/// Events — one per Qt Player signal + service ones.
#[derive(Clone, Debug)]
pub enum PlayerEvent {
    TrackChanged,
    StateChanged,
    PositionChanged,
    VolumeChanged,
    ModeChanged,
    QueueChanged,
    CoverFileChanged,
    ErrorChanged,
    LevelChanged,
    /// device/target/transfer/remote changed
    DeviceChanged,
    /// local engine: connecting/ready/error
    LocalChanged,
    Queue(QueueChange),
    TrackStarted(TrackStarted),
    TrackEnded(TrackEnded),
    /// a short message to the user ("Queue cleared", "Pixel 9 did not answer")
    Notice(String),
}

pub struct PlayerSubscription {
    pub snapshot: watch::Receiver<PlayerSnapshot>,
    pub events: broadcast::Receiver<PlayerEvent>,
}

#[derive(Debug, thiserror::Error)]
pub enum PlayerError {
    #[error("{0}")]
    Engine(String),
    #[error("Spotify Premium required")]
    PremiumRequired,
    #[error("no playback device")]
    NoDevice,
}

/// Codec signature: Ogg Vorbis at librespot's bitrate.
pub fn codec_label(bitrate_kbps: u16) -> String {
    format!("OGG {bitrate_kbps}")
}

/// librespot volume (0..=65535) ↔ 0..1 and ↔ percent (spotifast app.rs volume_to_percent/percent_to_volume).
pub fn volume_to_u16(volume: f64) -> u16 {
    (volume.clamp(0.0, 1.0) * f64::from(u16::MAX)).round() as u16
}

pub fn volume_from_u16(volume: u16) -> f64 {
    f64::from(volume) / f64::from(u16::MAX)
}
