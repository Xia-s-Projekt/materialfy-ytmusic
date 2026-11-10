//! AppContext - the interface between the shell ("shell") and the bridges ("bridges").
//! The shell creates it once (before QML type registration), the bridges only read it via [`ctx()`].
use std::path::PathBuf;
use std::sync::OnceLock;

use tokio::runtime::Handle;
use tokio::sync::watch;

/// Launch flags the bridges need.
#[derive(Clone, Debug, Default)]
pub struct StartupFlags {
    /// --screenshot: snapshot mode (no tray, MPRIS, session)
    pub screenshot: bool,
    /// --light: ThemeEngine.dark = false once after binding to settings (not written to settings)
    pub light: bool,
    /// --seed-image: ThemeEngine.setSeedFromImage(path) after binding to settings
    pub seed_image: Option<PathBuf>,
}

pub struct AppContext {
    /// tokio runtime (multi-threaded); the bridges do rt.spawn(...) and return to the GUI via CxxQtThread::queue
    pub rt: Handle,
    pub services: yt_services::Services,
    pub flags: StartupFlags,
    /// system dark theme (QStyleHints::colorScheme), updated by the shell on colorSchemeChanged
    pub system_dark: watch::Receiver<bool>,
}

static CONTEXT: OnceLock<AppContext> = OnceLock::new();

/// Called by the shell exactly once, before qt::ffi::register_qml_types().
pub fn init(context: AppContext) {
    if CONTEXT.set(context).is_err() {
        panic!("AppContext already created");
    }
}

/// Application context. Panics if the shell has not called init yet (a startup order error).
pub fn ctx() -> &'static AppContext {
    CONTEXT.get().expect("AppContext not created: call context::init() before registering QML types")
}
