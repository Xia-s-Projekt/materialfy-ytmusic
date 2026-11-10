//! The fastest monitor becomes the primary screen before windows are created (display.py). Linux only, C++
//! (cpp/mf_shell.cpp): QWindowSystemInterface::handlePrimaryScreenChanged(fastest->handle()) via dlsym -
//! if the symbol is missing, a skip with a message. MATERIALFY_KEEP_PRIMARY_SCREEN=1 - do not touch.
//! Windows/macOS - do nothing.

/// A line for the log (--debug-fps prints it to stderr).
pub fn prefer_fastest_screen() -> String {
    if cfg!(target_os = "linux") && std::env::var_os("MATERIALFY_KEEP_PRIMARY_SCREEN").is_some_and(|v| !v.is_empty()) {
        return "primary screen: kept the system one (MATERIALFY_KEEP_PRIMARY_SCREEN)".to_owned();
    }
    crate::qt::ffi::ffi::prefer_fastest_screen().to_string()
}
