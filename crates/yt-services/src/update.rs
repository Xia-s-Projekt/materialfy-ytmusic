//! Auto-update stub (keeps bridge API; full updater lives in materialfy).

use std::sync::Arc;

use tokio::runtime::Handle;
use tokio::sync::{broadcast, watch};

use mf_platform::SettingsStore;

use crate::AppRequest;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UpdateState {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available,
    Downloading,
    Verifying,
    ReadyToInstall,
    Installing,
    Error,
}
impl UpdateState {
    pub fn as_str(self) -> &'static str { self.code() }
    pub fn code(self) -> &'static str {
        match self {
            UpdateState::Idle => "idle",
            UpdateState::Checking => "checking",
            UpdateState::UpToDate => "upToDate",
            UpdateState::Available => "available",
            UpdateState::Downloading => "downloading",
            UpdateState::Verifying => "verifying",
            UpdateState::ReadyToInstall => "readyToInstall",
            UpdateState::Installing => "installing",
            UpdateState::Error => "error",
        }
    }
    pub fn busy(self) -> bool {
        matches!(self, UpdateState::Downloading | UpdateState::Verifying | UpdateState::Installing)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stage {
    #[default]
    Check,
    Download,
    Install,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Offer {
    pub version: String,
    pub notes: String,
    pub published_at: String,
    pub release_url: String,
    pub prerelease: bool,
    pub asset_name: String,
    pub asset_url: String,
    pub size: u64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct UpdateSnapshot {
    pub state: UpdateState,
    pub current_version: String,
    pub install_kind: String,
    pub offer: Option<Offer>,
    pub latest_version: String,
    pub release_notes: String,
    pub skipped: bool,
    pub manual: bool,
    pub downloaded: u64,
    pub total: u64,
    pub speed: f64,
    pub eta: i64,
    pub error_text: String,
    pub retryable: bool,
    pub error_stage: Stage,
    pub last_checked: i64,
    pub note: String,
}

pub struct UpdateService {
    _rt: Handle,
    _settings: Arc<SettingsStore>,
    _app_requests: broadcast::Sender<AppRequest>,
    snapshot: watch::Sender<UpdateSnapshot>,
}

impl UpdateService {
    pub fn new(rt: Handle, settings: Arc<SettingsStore>, app_requests: broadcast::Sender<AppRequest>) -> Arc<Self> {
        let (snapshot, _) = watch::channel(UpdateSnapshot { current_version: env!("CARGO_PKG_VERSION").to_string(), ..Default::default() });
        Arc::new(Self { _rt: rt, _settings: settings, _app_requests: app_requests, snapshot })
    }
    pub fn check(&self, _force: bool) {}
    pub fn download(&self) {}
    pub fn cancel(&self) {}
    pub fn retry(&self) {}
    pub fn install(&self) {}
    pub fn skip(&self) {}
    pub fn open_release_page(&self) { let _ = yt_auth::open_browser("https://github.com/Xia-s-Projekt/materialfy-ytmusic/releases"); }
    pub fn install_kind(&self) -> crate::update::InstallKindStub { crate::update::InstallKindStub }
    pub fn start_schedule(&self, _delay: std::time::Duration) {}
    pub fn snapshot(&self) -> UpdateSnapshot { self.snapshot.borrow().clone() }
    pub fn watch(&self) -> watch::Receiver<UpdateSnapshot> { self.snapshot.subscribe() }
}

pub mod watchers {}

pub const RELEASES_PAGE: &str = "https://github.com/Xia-s-Projekt/materialfy-ytmusic/releases";
pub const START_DELAY: std::time::Duration = std::time::Duration::from_secs(10);
pub const CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(6 * 3600);
pub const FAKE_VERSION_ENV: &str = "MATERIALFY_UPDATE_FAKE_VERSION";

pub fn fake_version_set() -> bool {
    std::env::var(FAKE_VERSION_ENV).is_ok()
}

pub mod progress {
    pub fn fraction(done: u64, total: u64) -> f64 {
        if total == 0 { 0.0 } else { (done as f64 / total as f64).clamp(0.0, 1.0) }
    }
    pub fn eta_secs(done: u64, total: u64, rate: Option<f64>) -> Option<u64> {
        let rate = rate.filter(|r| r.is_finite() && *r >= 1.0)?;
        if total == 0 { return None; }
        let left = total.saturating_sub(done) as f64;
        Some((left / rate).ceil() as u64)
    }
}

#[derive(Clone, Debug, Default)]
pub struct InstallKindStub;
impl InstallKindStub {
    pub fn code(&self) -> &'static str { "dev" }
}
