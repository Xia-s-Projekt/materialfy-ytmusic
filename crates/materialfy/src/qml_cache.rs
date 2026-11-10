//! Compiled QML cache (written by Qt itself, QML_DISK_CACHE_PATH): its own folder per application version and a reset
//! after a failed launch. A corrupted cache file (process killed mid-write) is taken by Qt as-is - the content
//! is not checked, - and the QML engine crashes on every start (Qt6Qml.dll, 0xc0000005 in 0.3.1 for a user).
//! The "launch not finished" marker is cleared after 15 s of running; if we find it at startup - the previous launch crashed, the cache
//! is erased, QML will compile again.
use std::path::{Path, PathBuf};
use std::time::Duration;

const SETTLE: Duration = Duration::from_secs(15);

pub struct StartGuard {
    marker: PathBuf,
}

/// Call before creating QApplication (sets QML_DISK_CACHE_PATH). `guard` - watch for failed launches
/// (not in screenshot mode: there the process exits before the marker is cleared). None - the user set the cache path.
pub fn prepare(guard: bool) -> Option<StartGuard> {
    if std::env::var_os("QML_DISK_CACHE_PATH").is_some() {
        return None;
    }
    let root = mf_platform::dirs::cache_dir().join("qmlcache");
    let version = env!("CARGO_PKG_VERSION");
    let dir = root.join(version);
    remove_other_versions(&root, version);
    let marker = root.join(format!("starting-{version}"));
    if guard && marker.exists() {
        tracing::warn!("the previous start did not finish: the compiled QML cache is cleared");
        let _ = std::fs::remove_dir_all(&dir);
    }
    if let Err(error) = std::fs::create_dir_all(&dir) {
        tracing::warn!("QML cache {}: {error}", dir.display());
        return None;
    }
    // via Qt: on Windows it reads variables from a copy of the C runtime, std::env::set_var does not change it
    crate::qt::ffi::ffi::set_qt_env(
        &cxx_qt_lib::QString::from("QML_DISK_CACHE_PATH"),
        &cxx_qt_lib::QString::from(dir.to_string_lossy().as_ref()),
    );
    guard.then_some(StartGuard { marker })
}

impl StartGuard {
    /// Set the marker (once it is clear that this process really is launching a window) and clear it after SETTLE of running.
    pub fn arm(self, rt: &tokio::runtime::Handle) {
        let _ = std::fs::write(&self.marker, b"");
        rt.spawn(async move {
            tokio::time::sleep(SETTLE).await;
            let _ = std::fs::remove_file(&self.marker);
        });
    }
}

/// The cache of past versions and the old flat cache (files directly in qmlcache, before 0.3.2) - delete.
fn remove_other_versions(root: &Path, version: &str) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if name != version {
                let _ = std::fs::remove_dir_all(&path);
            }
        } else if name.ends_with(".qmlc") || name.ends_with(".jsc") || (name.starts_with("starting-") && name != format!("starting-{version}")) {
            let _ = std::fs::remove_file(&path);
        }
    }
}
