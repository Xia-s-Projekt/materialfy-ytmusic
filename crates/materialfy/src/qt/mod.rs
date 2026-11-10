//! Qt helpers for the bridges (owned by the "shell"). Call everything only from the Qt GUI thread.
pub mod ffi;
pub mod variant;

use std::sync::OnceLock;

use cxx_qt::QObject;
use cxx_qt_lib::QString;
use tokio::sync::watch;

pub use variant::{
    QVariantList, QVariantMap, json_to_qvariant, json_to_qvariantlist, json_to_qvariantmap, qvariant_to_json,
    qvariantmap_to_json,
};

/// Text into the system clipboard.
pub fn copy_to_clipboard(text: &str) {
    ffi::ffi::copy_to_clipboard(&QString::from(text));
}

/// A QObject handed to QML from Q_INVOKABLE stays owned by Rust/C++.
/// # Safety
/// `object` is a live QObject (or null).
pub unsafe fn set_cpp_ownership(object: *mut QObject) {
    unsafe { ffi::ffi::set_cpp_ownership(object) }
}

/// System dark theme (QStyleHints::colorScheme() != Light).
pub fn system_dark() -> bool {
    ffi::ffi::system_color_scheme_dark()
}

/// Open a directory in the file manager (QDesktopServices). false - the system could not.
pub fn open_local_folder(path: &std::path::Path) -> bool {
    ffi::ffi::open_local_folder(&QString::from(path.to_string_lossy().as_ref()))
}

static SYSTEM_DARK: OnceLock<watch::Sender<bool>> = OnceLock::new();

/// The system theme channel for AppContext::system_dark: the initial value is [`system_dark`],
/// after that it is updated on QStyleHints::colorSchemeChanged. Call after creating QApplication.
pub fn system_dark_watch() -> watch::Receiver<bool> {
    let mut first = false;
    let sender = SYSTEM_DARK.get_or_init(|| {
        first = true;
        watch::Sender::new(system_dark())
    });
    if first {
        ffi::ffi::watch_color_scheme();
    }
    sender.subscribe()
}

pub(crate) fn set_system_dark(dark: bool) {
    if let Some(sender) = SYSTEM_DARK.get() {
        sender.send_if_modified(|value| std::mem::replace(value, dark) != dark);
    }
}
