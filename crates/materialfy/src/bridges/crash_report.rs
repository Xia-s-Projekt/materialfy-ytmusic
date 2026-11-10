//! CrashReport: the previous launch crashed or hung - new entries appeared in crash.log (main.rs, cpp/mf_shell.cpp).
//! The window at startup offers to report it: a folder with the log and a new GitHub issue with a text template.
//! "Viewed" - the size and time of the crash.log.seen file next to it.
use std::path::PathBuf;
use std::pin::Pin;

use cxx_qt::CxxQtType;
use cxx_qt_lib::QString;

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    unsafe extern "RustQt" {
        #[qobject]
        /// crash.log has entries the user has not seen yet
        #[qproperty(bool, pending, READ, NOTIFY = pending_changed)]
        /// file:/// URL of the folder with crash.log
        #[qproperty(QString, folder, READ, CONSTANT)]
        /// a new GitHub issue: the version and OS are already filled in
        #[qproperty(QString, issue_url, cxx_name = "issueUrl", READ, CONSTANT)]
        type CrashReport = super::CrashReportRust;

        #[qsignal]
        #[cxx_name = "pendingChanged"]
        fn pending_changed(self: Pin<&mut CrashReport>);

        /// Mark the current entries as viewed.
        #[qinvokable]
        fn dismiss(self: Pin<&mut CrashReport>);
    }
}

pub struct CrashReportRust {
    pending: bool,
    folder: QString,
    issue_url: QString,
}

fn log_path() -> PathBuf {
    mf_platform::dirs::app_data_dir().join("crash.log")
}

fn seen_path() -> PathBuf {
    mf_platform::dirs::app_data_dir().join("crash.log.seen")
}

/// A log fingerprint: size and modification time (the log is truncated to 64 KiB, so size alone is not enough).
fn stamp() -> Option<String> {
    let meta = std::fs::metadata(log_path()).ok().filter(|meta| meta.len() > 0)?;
    let modified = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
    Some(format!("{} {modified}", meta.len()))
}

fn percent_encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (byte as char).to_string(),
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

fn issue_url() -> String {
    let version = env!("CARGO_PKG_VERSION");
    let title = format!("Crash report ({version}, {})", std::env::consts::OS);
    let body = format!(
        "**What were you doing when it happened?**\n\n\n\
         **crash.log**: drag the file from the folder Materialfy opened into this box.\n\n\
         Materialfy {version}, {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    format!(
        "https://github.com/fatyzzz/materialfy/issues/new?title={}&body={}",
        percent_encode(&title),
        percent_encode(&body)
    )
}

impl Default for CrashReportRust {
    fn default() -> Self {
        let seen = std::fs::read_to_string(seen_path()).unwrap_or_default();
        let pending = stamp().is_some_and(|stamp| stamp != seen.trim());
        let folder = mf_platform::dirs::app_data_dir();
        let folder = format!("file:///{}", folder.to_string_lossy().replace('\\', "/").trim_start_matches('/'));
        Self { pending, folder: QString::from(folder.as_str()), issue_url: QString::from(issue_url().as_str()) }
    }
}

impl qobject::CrashReport {
    fn dismiss(mut self: Pin<&mut Self>) {
        if let Some(stamp) = stamp() {
            let _ = std::fs::write(seen_path(), stamp);
        }
        if self.pending {
            self.as_mut().rust_mut().pending = false;
            self.as_mut().pending_changed();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_issue_link_is_encoded() {
        assert_eq!(percent_encode("a b/é"), "a%20b%2F%C3%A9");
        let url = issue_url();
        assert!(url.starts_with("https://github.com/fatyzzz/materialfy/issues/new?title=Crash%20report%20%28"));
        assert!(!url.contains(' ') && !url.contains('\n'));
    }
}
