// Materialfy - a Spotify client: QML (src/materialfy/qml) + a Rust backend (cxx-qt, librespot).
// Shell: cli, context, display, engine, icons, images, models, qt, winframe; QML bridges - bridges.
// the release on Windows - the GUI subsystem, without a black console window; the debug build keeps the console for logs
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod bridges;
mod cli;
mod context;
mod display;
mod engine;
mod icons;
mod images;
mod models;
mod qml_cache;
mod qt;
mod winframe;

fn main() {
    install_crash_log();
    let args: Vec<String> = std::env::args_os().map(|a| a.to_string_lossy().into_owned()).collect();
    let code = engine::run(cli::Cli::parse(&args));
    std::process::exit(code);
}

/// A panic in the GUI thread (or in code that calls Qt) brings the process down without a trace: the Windows build has no console.
/// We write it to <app_data_dir>/crash.log (the last ~64 KiB) - with the version, thread, place and call stack.
fn install_crash_log() {
    let log = mf_platform::dirs::app_data_dir().join("crash.log");
    let _ = std::fs::create_dir_all(mf_platform::dirs::app_data_dir());
    // crashes in native code (Qt, the video driver) - go there too, on Windows and Linux
    qt::ffi::ffi::install_native_crash_log(&cxx_qt_lib::QString::from(log.to_string_lossy().as_ref()));
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let path = mf_platform::dirs::app_data_dir().join("crash.log");
        let thread = std::thread::current();
        let entry = format!(
            "=== {} · Materialfy {} ({}) · thread {}
{info}
{}
",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|since| since.as_secs())
                .unwrap_or_default(),
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            thread.name().unwrap_or("?"),
            std::backtrace::Backtrace::force_capture(),
        );
        let previous = std::fs::read_to_string(&path).unwrap_or_default();
        let keep = previous.len().saturating_sub(64 * 1024);
        let start = previous.char_indices().map(|(i, _)| i).find(|&i| i >= keep).unwrap_or(previous.len());
        let _ = std::fs::create_dir_all(path.parent().unwrap_or(std::path::Path::new(".")));
        let _ = std::fs::write(&path, format!("{}{entry}", &previous[start..]));
        default(info);
    }));
}
