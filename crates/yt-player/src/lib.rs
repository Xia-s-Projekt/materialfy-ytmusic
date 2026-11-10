//! YouTube Music player without Qt, replacing librespot (sf-player).
//!
//! Playback: InnerTube `player` (anonymous ANDROID client) → plain CDN URL →
//! `rodio` decodes AAC while a background thread streams the bytes with HTTP
//! Range requests. No yt-dlp, no Python, no full-file download.
//! No Spotify Connect, no mDNS, no remote devices: this computer only.
//!
//! Symphonia (via rodio 0.23) decodes the AAC stream. Opus is not decoded by
//! symphonia, so `yt-api::innertube` only picks MP4 formats; a response with
//! nothing else surfaces through `error_text`.

pub mod audio;
pub mod handle;
pub mod stream;

pub use handle::PlayerHandle;
pub use stream::output_devices;

pub struct PlayerSubscription {
    pub snapshot: tokio::sync::watch::Receiver<PlayerSnapshot>,
    pub events: tokio::sync::broadcast::Receiver<PlayerEvent>,
}

use md3_model::{DeviceItem, PlayContext, QueueRow, TrackItem};

pub const REMOTE_POLL_ACTIVE_S: u64 = 4;
pub const REMOTE_POLL_IDLE_S: u64 = 20;
pub const REMOTE_POLL_UNSEEN_S: u64 = 30;
pub const OPTIMISTIC_HOLD_MS: u64 = 2500;
pub const PLAYBACK_HOLD_MS: u64 = 6000;
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
    All,
    One,
}

impl Repeat {
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
    pub fn cycled(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Target {
    #[default]
    Local,
    Remote(Option<String>),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum LocalPlayback {
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

#[derive(Clone)]
pub struct PlayerConfig {
    pub cover_dir: std::path::PathBuf,
    pub session_file: Option<std::path::PathBuf>,
    pub volume: f64,
    pub http: reqwest::Client,
}

#[derive(Clone, Debug, Default)]
pub struct PlayerSnapshot {
    pub track: Option<TrackItem>,
    pub codec: String,
    pub cover_file: String,
    pub playing: bool,
    pub buffering: bool,
    pub narrating: bool,
    pub position_ms: i64,
    pub duration_ms: i64,
    pub volume: f64,
    pub shuffle: bool,
    pub repeat: Repeat,
    pub current_index: i32,
    pub error_text: String,
    pub direction: i32,
    pub level: f64,
    pub levels_wanted: bool,
    pub context: PlayContext,
    pub queue: std::sync::Arc<Vec<QueueRow>>,
    pub played_seconds: f64,
    pub target: Target,
    pub device: Option<DeviceItem>,
    pub is_local: bool,
    pub local: LocalPlayback,
    pub can_control: bool,
    pub can_set_volume: bool,
    pub transferring: bool,
    pub device_lost: bool,
    pub context_remaining: i32,
    pub manual_count: i32,
    pub can_clear_queue: bool,
    pub queue_reorderable: bool,
}

impl PlayerSnapshot {
    pub fn has_track(&self) -> bool {
        self.track.is_some()
    }
    pub fn source(&self) -> &str {
        &self.context.kind
    }
    pub fn position(&self) -> f64 {
        if self.duration_ms > 0 { self.position_ms as f64 / self.duration_ms as f64 } else { 0.0 }
    }
    pub fn is_episode(&self) -> bool {
        false
    }
}

#[derive(Clone, Debug)]
pub enum QueueChange {
    Reset(std::sync::Arc<Vec<QueueRow>>),
    Insert { at: usize, rows: Vec<QueueRow> },
    Remove { at: usize, count: usize },
    Move { from: usize, to: usize },
}

#[derive(Clone, Debug)]
pub struct TrackEnded {
    pub track: TrackItem,
    pub context: PlayContext,
    pub played: f64,
    pub end: f64,
    pub natural: bool,
    pub local: bool,
}

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
    DeviceChanged,
    Queue(QueueChange),
    TrackEnded(TrackEnded),
    Notice(String),
}

#[derive(Debug, thiserror::Error)]
pub enum PlayerError {
    #[error("{0}")]
    Engine(String),
    #[error("YouTube Music sign-in required")]
    PremiumRequired,
    #[error("no playback device")]
    NoDevice,
}

pub fn codec_label(bitrate_kbps: u16) -> String {
    format!("AAC {bitrate_kbps}k")
}

pub fn volume_to_u16(volume: f64) -> u16 {
    (volume.clamp(0.0, 1.0) * f64::from(u16::MAX)).round() as u16
}

pub fn volume_from_u16(volume: u16) -> f64 {
    f64::from(volume) / f64::from(u16::MAX)
}
