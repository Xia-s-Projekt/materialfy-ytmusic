//! Auto-update from GitHub Releases of this fork (Xia-s-Projekt/materialfy-ytmusic).
//!
//! States: idle → checking → (upToDate | available) → downloading (progress)
//! → verifying → readyToInstall → installing → idle. install_kind is "portable"
//! for the in-place Linux install (the binary is swapped and the app restarts),
//! "manual" for other platforms (the release page is opened instead).

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::runtime::Handle;
use tokio::sync::{broadcast, watch};

use mf_platform::{SettingKey, SettingsStore};

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
    rt: Handle,
    settings: Arc<SettingsStore>,
    app_requests: broadcast::Sender<AppRequest>,
    snapshot: watch::Sender<UpdateSnapshot>,
    /// The in-flight download task, if any.
    download: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl UpdateService {
    pub fn new(rt: Handle, settings: Arc<SettingsStore>, app_requests: broadcast::Sender<AppRequest>) -> Arc<Self> {
        let (snapshot, _) = watch::channel(UpdateSnapshot { current_version: current_version(), install_kind: install_kind().to_string(), ..Default::default() });
        Arc::new(Self { rt, settings, app_requests, snapshot, download: Arc::new(Mutex::new(None)) })
    }

    fn set(&self, mutate: impl FnOnce(&mut UpdateSnapshot)) {
        let mut next = self.snapshot.borrow().clone();
        mutate(&mut next);
        let _ = self.snapshot.send_replace(next);
    }

    pub fn check(&self, force: bool) {
        if self.snapshot.borrow().state == UpdateState::Checking {
            return;
        }
        let _ = force;
        let snapshot = self.snapshot.clone();
        let settings = self.settings.clone();
        self.set(|s| {
            s.state = UpdateState::Checking;
            s.error_text.clear();
            s.retryable = false;
        });
        self.rt.spawn(async move {
            run_check(snapshot, &settings).await;
        });
    }

    pub fn download(&self) {
        let Some(offer) = self.snapshot.borrow().offer.clone() else { return };
        if offer.asset_url.is_empty() {
            self.set(|s| {
                s.state = UpdateState::Error;
                s.error_text = "This release has no downloadable file for your system".to_string();
                s.retryable = false;
                s.error_stage = Stage::Download;
            });
            return;
        }
        self.cancel();
        let snapshot = self.snapshot.clone();
        let version = offer.version.clone();
        let url = offer.asset_url.clone();
        let total = if offer.size > 0 { offer.size } else { u64::MAX };
        let dir = std::env::temp_dir().join(format!("materialfy-update-{version}"));
        self.set(|s| {
            s.state = UpdateState::Downloading;
            s.downloaded = 0;
            s.total = total;
            s.speed = 0.0;
            s.eta = -1;
            s.error_text.clear();
            s.retryable = false;
        });
        let task = self.rt.spawn(async move {
            let _ = std::fs::create_dir_all(&dir);
            let out_path = dir.join("asset");
            let http = reqwest::Client::new();
            let started = std::time::Instant::now();
            let mut last_emit = started;
            let mut downloaded: u64 = 0;
            let mut result: Result<(), String> = Ok(());
            match http.get(&url).send().await {
                Ok(response) => {
                    let total = response.content_length().unwrap_or(total);
                    let mut response = response;
                    let mut file = match tokio::fs::File::create(&out_path).await {
                        Ok(f) => f,
                        Err(e) => {
                            let _ = snapshot.send_replace(UpdateSnapshot {
                                state: UpdateState::Error,
                                current_version: current_version(),
                                install_kind: install_kind().to_string(),
                                error_text: format!("could not create the download file: {e}"),
                                retryable: true,
                                error_stage: Stage::Download,
                                last_checked: now_unix(),
                                downloaded,
                                ..Default::default()
                            });
                            return;
                        }
                    };
                    use tokio::io::AsyncWriteExt;
                    loop {
                        match response.chunk().await {
                            Ok(Some(bytes)) => {
                                let len = bytes.len() as u64;
                                if let Err(e) = file.write(&bytes).await {
                                    result = Err(format!("writing the download: {e}"));
                                    break;
                                }
                                downloaded += len;
                                if last_emit.elapsed() >= Duration::from_millis(200) {
                                    last_emit = std::time::Instant::now();
                                    let elapsed = started.elapsed().as_secs_f64().max(0.001);
                                    let speed = downloaded as f64 / elapsed;
                                    let eta = progress::eta_secs(downloaded, total, Some(speed)).map(|e| e as i64).unwrap_or(-1);
                                    let _ = snapshot.send_replace(UpdateSnapshot {
                                        state: UpdateState::Downloading,
                                        current_version: current_version(),
                                        install_kind: install_kind().to_string(),
                                        downloaded,
                                        total: if total == u64::MAX { 0 } else { total },
                                        speed,
                                        eta,
                                        offer: Some(offer.clone()),
                                        last_checked: now_unix(),
                                        ..Default::default()
                                    });
                                }
                            }
                            Ok(None) => break,
                            Err(e) => {
                                result = Err(format!("download failed: {e}"));
                                break;
                            }
                        }
                    }
                    let _ = file.flush().await;
                }
                Err(e) => result = Err(format!("download failed: {e}")),
            }
            match result {
                Ok(()) => {
                    // Verify the download is complete (size, when known).
                    let actual = std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
                    if total != u64::MAX && actual != total {
                        let _ = snapshot.send_replace(UpdateSnapshot {
                            state: UpdateState::Error,
                            current_version: current_version(),
                            install_kind: install_kind().to_string(),
                            error_text: "the download is incomplete (size mismatch)".to_string(),
                            retryable: true,
                            error_stage: Stage::Download,
                            last_checked: now_unix(),
                            downloaded,
                            offer: Some(offer.clone()),
                            ..Default::default()
                        });
                    } else {
                        let _ = snapshot.send_replace(UpdateSnapshot {
                            state: UpdateState::ReadyToInstall,
                            current_version: current_version(),
                            install_kind: install_kind().to_string(),
                            downloaded: actual,
                            total: actual,
                            offer: Some(offer.clone()),
                            last_checked: now_unix(),
                            ..Default::default()
                        });
                    }
                }
                Err(message) => {
                    let _ = snapshot.send_replace(UpdateSnapshot {
                        state: UpdateState::Error,
                        current_version: current_version(),
                        install_kind: install_kind().to_string(),
                        error_text: message,
                        retryable: true,
                        error_stage: Stage::Download,
                        last_checked: now_unix(),
                        downloaded,
                        offer: Some(offer.clone()),
                        ..Default::default()
                    });
                }
            }
        });
        *self.download.lock().unwrap_or_else(|p| p.into_inner()) = Some(task);
    }

    pub fn cancel(&self) {
        if let Some(task) = self.download.lock().unwrap_or_else(|p| p.into_inner()).take() {
            task.abort();
        }
        if self.snapshot.borrow().state.busy() {
            self.set(|s| {
                s.state = if s.offer.is_some() { UpdateState::Available } else { UpdateState::Idle };
                s.downloaded = 0;
                s.speed = 0.0;
                s.eta = -1;
            });
        }
    }

    pub fn retry(&self) {
        match self.snapshot.borrow().error_stage {
            Stage::Check => self.check(true),
            Stage::Download => self.download(),
            Stage::Install => self.install(),
        }
    }

    pub fn install(&self) {
        if install_kind() != "portable" {
            self.set(|s| {
                s.state = UpdateState::Error;
                s.error_text = "This build can't install updates itself; download the new version from the release page".to_string();
                s.retryable = false;
                s.error_stage = Stage::Install;
            });
            return;
        }
        let Some(offer) = self.snapshot.borrow().offer.clone() else { return };
        let version = offer.version.clone();
        let snapshot = self.snapshot.clone();
        let app_requests = self.app_requests.clone();
        self.set(|s| {
            s.state = UpdateState::Installing;
            s.error_text.clear();
            s.retryable = false;
        });
        self.rt.spawn(async move {
            let downloaded = std::env::temp_dir().join(format!("materialfy-update-{version}")).join("asset");
            match install_portable(&downloaded).await {
                Ok(()) => {
                    let _ = snapshot.send_replace(UpdateSnapshot {
                        state: UpdateState::Installing,
                        current_version: current_version(),
                        install_kind: install_kind().to_string(),
                        note: "Restarting…".to_string(),
                        ..Default::default()
                    });
                    let _ = app_requests.send(AppRequest::Quit);
                }
                Err(e) => {
                    let _ = snapshot.send_replace(UpdateSnapshot {
                        state: UpdateState::Error,
                        current_version: current_version(),
                        install_kind: install_kind().to_string(),
                        error_text: e,
                        retryable: true,
                        error_stage: Stage::Install,
                        last_checked: now_unix(),
                        ..Default::default()
                    });
                }
            }
        });
    }

    pub fn skip(&self) {
        let current = self.snapshot.borrow();
        if let Some(offer) = &current.offer {
            let _ = self.settings.set(SettingKey::SkippedVersion, mf_platform::SettingValue::Str(offer.version.clone()));
        }
        drop(current);
        self.set(|s| {
            s.skipped = true;
        });
        let _ = self.snapshot.send_replace(UpdateSnapshot {
            state: UpdateState::UpToDate,
            current_version: current_version(),
            install_kind: install_kind().to_string(),
            skipped: true,
            last_checked: now_unix(),
            ..Default::default()
        });
    }

    pub fn open_release_page(&self) { let _ = yt_auth::open_browser(RELEASES_PAGE); }

    /// "portable" for an in-place Linux install (bin/materialfy), "manual" elsewhere.
    pub fn install_kind(&self) -> InstallKind {
        InstallKind
    }

    pub fn start_schedule(&self, delay: Duration) {
        let settings = self.settings.clone();
        let snapshot = self.snapshot.clone();
        self.rt.spawn(async move {
            tokio::time::sleep(delay).await;
            loop {
                if settings.get_bool(SettingKey::AutoUpdateCheck) {
                    run_check(snapshot.clone(), &settings).await;
                }
                tokio::time::sleep(CHECK_INTERVAL).await;
            }
        });
    }

    pub fn snapshot(&self) -> UpdateSnapshot { self.snapshot.borrow().clone() }
    pub fn watch(&self) -> watch::Receiver<UpdateSnapshot> { self.snapshot.subscribe() }
}

// ── helpers ────────────────────────────────────────────────────────────────

/// One check against the GitHub releases feed. Writes the resulting state.
async fn run_check(snapshot: watch::Sender<UpdateSnapshot>, settings: &SettingsStore) {
    let channel = settings.get_str(SettingKey::UpdateChannel);
    let skipped_version = settings.get_str(SettingKey::SkippedVersion);
    match fetch_latest(&channel).await {
        Ok(Some(offer)) => {
            let is_newer = is_newer_than_current(&offer.version);
            let skipped = !skipped_version.is_empty() && skipped_version == offer.version;
            let _ = snapshot.send_replace(UpdateSnapshot {
                state: if is_newer { UpdateState::Available } else { UpdateState::UpToDate },
                current_version: current_version(),
                install_kind: install_kind().to_string(),
                latest_version: offer.version.clone(),
                release_notes: offer.notes.clone(),
                skipped,
                last_checked: now_unix(),
                offer: if is_newer { Some(offer) } else { None },
                ..Default::default()
            });
        }
        Ok(None) => {
            let _ = snapshot.send_replace(UpdateSnapshot {
                state: UpdateState::UpToDate,
                current_version: current_version(),
                install_kind: install_kind().to_string(),
                last_checked: now_unix(),
                ..Default::default()
            });
        }
        Err(e) => {
            let _ = snapshot.send_replace(UpdateSnapshot {
                state: UpdateState::Error,
                current_version: current_version(),
                install_kind: install_kind().to_string(),
                error_text: e,
                retryable: true,
                error_stage: Stage::Check,
                last_checked: now_unix(),
                ..Default::default()
            });
        }
    }
}

pub struct InstallKind;
impl InstallKind {
    pub fn code(&self) -> &'static str { install_kind() }
}

fn install_kind() -> &'static str {
    if cfg!(target_os = "linux") { "portable" } else { "manual" }
}

fn current_version() -> String {
    if let Ok(fake) = std::env::var(FAKE_VERSION_ENV) {
        fake
    } else {
        env!("CARGO_PKG_VERSION").to_string()
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// "v0.6.0" → (0, 6, 0); non-numeric → (0, 0, 0).
fn parse_version(tag: &str) -> (u64, u64, u64) {
    let s = tag.trim().trim_start_matches('v');
    let mut parts = s.split('.').map(|p| p.parse::<u64>().unwrap_or(0));
    (parts.next().unwrap_or(0), parts.next().unwrap_or(0), parts.next().unwrap_or(0))
}

fn is_newer_than_current(tag: &str) -> bool {
    parse_version(tag) > parse_version(&current_version())
}

/// Linux asset for this machine, if the release carries one.
fn pick_asset(release: &serde_json::Value) -> Option<(String, u64)> {
    let arch = std::env::consts::ARCH;
    let want = match arch {
        "x86_64" => "linux-x86_64",
        "aarch64" => "linux-aarch64",
        _ => return None,
    };
    release
        .get("assets")
        .and_then(|a| a.as_array())?
        .iter()
        .find(|a| a.get("name").and_then(|n| n.as_str()).is_some_and(|n| n.contains(want)))
        .map(|a| (a.get("browser_download_url").and_then(|u| u.as_str()).unwrap_or("").to_string(), a.get("size").and_then(|s| s.as_u64()).unwrap_or(0)))
}

async fn fetch_latest(channel: &str) -> Result<Option<Offer>, String> {
    let client = reqwest::Client::new();
    let resp = client
        .get("https://api.github.com/repos/Xia-s-Projekt/materialfy-ytmusic/releases?per_page=20")
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "materialfy-updater")
        .send()
        .await
        .map_err(|e| format!("could not reach the update server: {e}"))?;
    let releases: Vec<serde_json::Value> = resp
        .json()
        .await
        .map_err(|e| format!("the update server sent an unreadable response: {e}"))?;
    let beta = channel == "beta";
    let release = releases.iter().find(|r| beta || !r.get("prerelease").and_then(|p| p.as_bool()).unwrap_or(false));
    let Some(release) = release else { return Ok(None) };
    let tag = release.get("tag_name").and_then(|t| t.as_str()).unwrap_or("").to_string();
    if tag.is_empty() {
        return Ok(None);
    }
    let (asset_url, size) = pick_asset(release).unwrap_or_default();
    let asset_name = release
        .get("assets")
        .and_then(|a| a.as_array())
        .and_then(|assets| assets.iter().find(|a| a.get("browser_download_url").and_then(|u| u.as_str()) == Some(&asset_url)))
        .and_then(|a| a.get("name").and_then(|n| n.as_str()).map(str::to_string))
        .unwrap_or_default();
    Ok(Some(Offer {
        version: tag,
        notes: release.get("body").and_then(|b| b.as_str()).unwrap_or("").to_string(),
        published_at: release.get("published_at").and_then(|b| b.as_str()).unwrap_or("").to_string(),
        release_url: release.get("html_url").and_then(|b| b.as_str()).unwrap_or(RELEASES_PAGE).to_string(),
        prerelease: release.get("prerelease").and_then(|p| p.as_bool()).unwrap_or(false),
        asset_name,
        asset_url,
        size,
    }))
}

/// Replace the running executable with the downloaded one (Linux, portable
/// layout) and start it again. Only called from install_kind() == "portable".
async fn install_portable(downloaded: &Path) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("could not find this executable: {e}"))?;
    // Extract the tar.gz (or use it directly if it is the bare binary).
    let dir = downloaded.parent().ok_or_else(|| "bad install path".to_string())?;
    let extracted = dir.join("materialfy");
    if downloaded.extension().is_some_and(|e| e == "gz" || e == "tgz") {
        let status = tokio::process::Command::new("tar")
            .arg("xzf")
            .arg(downloaded)
            .arg("-C")
            .arg(dir)
            .status()
            .await
            .map_err(|e| format!("could not unpack the update: {e}"))?;
        if !status.success() {
            return Err("could not unpack the update".to_string());
        }
    } else {
        std::fs::copy(downloaded, &extracted).map_err(|e| format!("could not copy the update: {e}"))?;
    }
    if !extracted.is_file() {
        return Err("the update archive did not contain the materialfy binary".to_string());
    }
    // Stage next to the running exe so the rename is on the same filesystem.
    let staged = exe.with_extension("new");
    std::fs::copy(&extracted, &staged).map_err(|e| format!("could not stage the update: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755));
    }
    // On Linux the running process keeps its open inode; the rename is safe.
    std::fs::rename(&staged, &exe).map_err(|e| format!("could not replace this executable ({e}); replace it manually from the release page"))?;
    // Start the new build after this one exits (single-instance guard would
    // bounce a second copy while this process is still alive).
    std::process::Command::new("sh")
        .args(["-c", "sleep 2; exec \"$0\" \"$@\"", exe.to_string_lossy().as_ref()])
        .spawn()
        .map_err(|e| format!("could not restart the app: {e}"))?;
    Ok(())
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
