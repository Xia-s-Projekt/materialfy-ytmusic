//! Помощники Qt для мостов (владелец — «shell»). Всё вызывать только из GUI-потока Qt.
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

/// Текст в системный буфер обмена.
pub fn copy_to_clipboard(text: &str) {
    ffi::ffi::copy_to_clipboard(&QString::from(text));
}

/// QObject, который отдаётся в QML из Q_INVOKABLE, остаётся во владении Rust/C++.
/// # Safety
/// `object` — живой QObject (или null).
pub unsafe fn set_cpp_ownership(object: *mut QObject) {
    unsafe { ffi::ffi::set_cpp_ownership(object) }
}

/// Системная тёмная тема (QStyleHints::colorScheme() != Light).
pub fn system_dark() -> bool {
    ffi::ffi::system_color_scheme_dark()
}

/// Открыть каталог в файловом менеджере (QDesktopServices). false — система не смогла.
pub fn open_local_folder(path: &std::path::Path) -> bool {
    ffi::ffi::open_local_folder(&QString::from(path.to_string_lossy().as_ref()))
}

static SYSTEM_DARK: OnceLock<watch::Sender<bool>> = OnceLock::new();

/// Канал системной темы для AppContext::system_dark: начальное значение — [`system_dark`],
/// дальше обновляется по QStyleHints::colorSchemeChanged. Вызывать после создания QApplication.
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
