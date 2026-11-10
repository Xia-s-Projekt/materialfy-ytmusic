//! Application startup - a port of main() from app.py. The order (it matters):
//! 1. env: remove QSG_USE_SIMPLE_ANIMATION_DRIVER; --debug-wheel -> QT_LOGGING_RULES; --screenshot ->
//!    QT_QPA_PLATFORM=offscreen, QT_QUICK_BACKEND=rhi, QSG_RHI_BACKEND defaults to d3d11 (Windows) / opengl;
//! 2. application names (Materialfy / materialfy / desktop "materialfy") and QApplication (qt::ffi::app_create);
//! 3. main mode and not --new-instance: single_instance::raise_running_instance() -> true - exit 0;
//! 4. window icon (assets/materialfy.svg), display::prefer_fastest_screen (before the first window), fonts assets/fonts/*.ttf;
//! 5. tracing-subscriber (materialfy: ..., level WARN; RUST_LOG overrides);
//! 6. tokio runtime; SettingsStore::open(config_dir); Services::start; context::init;
//! 7. qt::ffi::register_qml_types(); engine: NAM factory with a disk cache (images), import path = the qml directory,
//!    context properties materialfyDebugWheel, materialfyDebugFps, materialfyTray (= not a screenshot and not the gallery);
//! 8. initial properties (--size → width/height, --set), load(Main.qml | gallery/Gallery.qml | --qml);
//!    no root -> code 1; not --offline -> services.auth.start();
//! 9. if the root has the trayWanted property -> setQuitOnLastWindowClosed(false); Windows: winframe::install(window);
//!    install_memory_trim (in the tray / minimized - the scene and GPU resources are freed);
//!    watch_software_rendering (software rendering - the window has softwareRendering, decorative effects are off);
//!    AppRequest::RaiseWindow → show/raise/requestActivate, AppRequest::Quit → app_quit;
//! 10. --screenshot: after delay ms grabWindow().save(path) and exit;
//! 11. exec(); after that - services.shutdown().
//!
//! The QML directory: $MATERIALFY_QML_DIR, otherwise <exe>/qml, otherwise <repo>/src/materialfy/qml (searching upward from exe and cwd);
//! assets - the assets directory next to qml.
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use cxx_qt_lib::{QQmlApplicationEngine, QString, QUrl, QVariant};
use tokio::sync::broadcast::error::RecvError;
use mf_platform::{SettingKey, SettingsStore};
use yt_services::{AppRequest, Services, ServicesOptions};

use crate::cli::{Cli, InitialValue};
use crate::context::{self, AppContext, StartupFlags};
use crate::qt::QVariantMap;
use crate::qt::ffi::ffi;

fn qstr(text: &str) -> QString {
    QString::from(text)
}

fn qpath(path: &Path) -> QString {
    QString::from(path.to_string_lossy().as_ref())
}

fn env_set(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| !v.is_empty())
}

/// Environment variables before creating QApplication and any threads.
fn setup_env(cli: &Cli) {
    // SAFETY: called at the very start of main, there are no other threads yet
    unsafe {
        // The simple animation driver ticks with a ~60 Hz timer instead of vsync: at 144 Hz everything stutters
        std::env::remove_var("QSG_USE_SIMPLE_ANIMATION_DRIVER");
        if cli.debug_wheel {
            // the user's QT_LOGGING_RULES mutes console.log
            let rules = std::env::var("QT_LOGGING_RULES").unwrap_or_default();
            std::env::set_var("QT_LOGGING_RULES", format!("{rules};qml.debug=true"));
        }
        if cli.screenshot.is_some() {
            // A screenshot without a window on screen; RHI is needed for Shape/MultiEffect - the software backend does not draw them
            // QT_QPA_PLATFORM set by the caller wins (e.g. a bundle without the offscreen plugin)
            if std::env::var_os("QT_QPA_PLATFORM").is_none() {
                std::env::set_var("QT_QPA_PLATFORM", "offscreen");
            }
            std::env::set_var("QT_QUICK_BACKEND", "rhi");
            // Windows: D3D11 works offscreen; OpenGL may give a white screen
            if std::env::var_os("QSG_RHI_BACKEND").is_none() {
                let backend = if cfg!(target_os = "windows") { "d3d11" } else { "opengl" };
                std::env::set_var("QSG_RHI_BACKEND", backend);
            }
        }
    }
}

/// The discrete GPU vendor for configuring PRIME offload (Linux only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
enum GpuVendor {
    Nvidia,
    Other,
}

/// Detects the presence and vendor of a discrete GPU via sysfs.
/// On Linux, the presence of two or more
/// DRM render nodes (/dev/dri/renderD*) indicates multiple GPUs (hybrid graphics). Each node's vendor is read from
/// /sys/class/drm/<node>/device/vendor (for example, 0x10de - NVIDIA, 0x1002 - AMD, 0x8086 - Intel).
/// With the proprietary NVIDIA driver without nvidia-drm.modeset=1, a renderD* node is created only for
/// integrated graphics; in that case the presence of NVIDIA is indicated by the /sys/module/nvidia module.
fn discrete_gpu_vendor() -> Option<GpuVendor> {
    #[cfg(target_os = "linux")]
    {
        let entries = std::fs::read_dir("/dev/dri").ok()?;
        let mut render_nodes = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str
                .strip_prefix("renderD")
                .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()))
            {
                render_nodes.push(name_str.into_owned());
            }
        }

        let mut vendors = Vec::new();
        for node in &render_nodes {
            let path = format!("/sys/class/drm/{node}/device/vendor");
            if let Ok(content) = std::fs::read_to_string(&path) {
                let hex = content.trim();
                let hex = hex.strip_prefix("0x").or_else(|| hex.strip_prefix("0X")).unwrap_or(hex);
                if let Ok(vendor_id) = u32::from_str_radix(hex, 16) {
                    vendors.push(vendor_id);
                }
            }
        }

        if vendors.contains(&0x10de) {
            Some(GpuVendor::Nvidia)
        } else if vendors.contains(&0x1002) && render_nodes.len() >= 2 {
            Some(GpuVendor::Other)
        } else if Path::new("/sys/module/nvidia").exists() && vendors.iter().any(|&v| v != 0x10de) {
            Some(GpuVendor::Nvidia)
        } else {
            None
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// Applies the PRIME offload variables for the given discrete GPU vendor.
///
/// # Safety
/// Must be called strictly BEFORE other threads start.
unsafe fn apply_discrete_gpu(vendor: GpuVendor) {
    // SAFETY: called at the very start of the process in the GUI thread before other threads start
    unsafe {
        match vendor {
            GpuVendor::Nvidia => {
                // Redirect rendering to the discrete NVIDIA GPU (PRIME offload)
                std::env::set_var("__NV_PRIME_RENDER_OFFLOAD", "1");
                std::env::set_var("__GLX_VENDOR_LIBRARY_NAME", "nvidia");
                std::env::set_var("__VK_LAYER_NV_optimus", "NVIDIA_only");
                // Wayland: Qt takes the context via EGL, not GLX - the EGL vendor is also NVIDIA
                for json in ["/usr/share/glvnd/egl_vendor.d/10_nvidia.json", "/etc/glvnd/egl_vendor.d/10_nvidia.json"] {
                    if Path::new(json).exists() {
                        std::env::set_var("__EGL_VENDOR_LIBRARY_FILENAMES", json);
                        break;
                    }
                }
                std::env::set_var("DRI_PRIME", "1");
            }
            GpuVendor::Other => {
                // For AMD and other discrete GPUs, offload via Mesa is enough
                std::env::set_var("DRI_PRIME", "1");
            }
        }
    }
}

/// Applies the GPU preference via environment variables.
///
/// Must be called strictly BEFORE creating QApplication and the Qt graphics context (EGL/GLX/Vulkan),
/// otherwise the libglvnd / Mesa / NVIDIA driver loaders will fix the default device for the process.
fn apply_gpu_preference(settings: &SettingsStore) {
    let pref = settings.get_str(SettingKey::GpuPreference);
    match pref.as_str() {
        "integrated" => {
            // SAFETY: called at the very start of the process in the GUI thread before other threads start
            unsafe {
                // Force PRIME offload off and make it use integrated graphics
                std::env::remove_var("__NV_PRIME_RENDER_OFFLOAD");
                std::env::remove_var("__GLX_VENDOR_LIBRARY_NAME");
                std::env::remove_var("__VK_LAYER_NV_optimus");
                std::env::remove_var("DRI_PRIME");
                std::env::remove_var("__EGL_VENDOR_LIBRARY_FILENAMES");
            }
        }
        "discrete" => {
            // If there is no discrete GPU or the sysfs read failed - warn in the log
            let Some(vendor) = discrete_gpu_vendor() else {
                eprintln!("materialfy: gpu preference is 'discrete' but no dedicated GPU was detected");
                return;
            };
            // SAFETY: called at the very start of the process in the GUI thread before other threads start
            unsafe {
                apply_discrete_gpu(vendor);
            }
        }
        _ => {
            // "auto" - as the system decides: a music player has no need to wake the discrete GPU
            // (battery, fans); whoever hits the integrated driver's limits picks "Discrete" themselves
        }
    }
}

/// UI scale from settings: QT_SCALE_FACTOR is read by QGuiApplication at creation, therefore
/// the variable is set before app_create. A setting change is applied after the application restarts.
fn apply_ui_scale(settings: &SettingsStore) {
    const MARKER: &str = "MATERIALFY_UI_SCALE";
    // A restart from within the application (update, AppImage) inherits the previous process's variables:
    // in that case the setting takes priority over the inherited value, otherwise the new scale will not apply.
    let relaunched = std::env::var_os(MARKER).is_some();
    if !relaunched && std::env::var_os("QT_SCALE_FACTOR").is_some_and(|value| !value.is_empty()) {
        return; // an explicit user value takes priority over the setting
    }
    let percent = settings.get_int(SettingKey::UiScale);
    let factor = format!("{:.2}", percent as f64 / 100.0);
    let percent = percent.to_string();
    // Via Qt: on Windows it reads variables from a copy of the C runtime, std::env::set_var does not change it
    // (see qml_cache). std::env is needed so the restarted process inherits the value.
    ffi::set_qt_env(&qstr("QT_SCALE_FACTOR"), &qstr(&factor));
    // SAFETY: called before creating QApplication and other threads
    unsafe {
        std::env::set_var("QT_SCALE_FACTOR", &factor);
        std::env::set_var(MARKER, &percent);
    }
}

/// A temporary settings directory for --screenshot.
fn make_temp_dir() -> PathBuf {
    let base = std::env::temp_dir();
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    for attempt in 0..100u32 {
        let dir = base.join(format!("materialfy-shot-{:x}{:x}{attempt}", std::process::id(), stamp));
        if std::fs::create_dir(&dir).is_ok() {
            return dir;
        }
    }
    base
}

fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

fn load_fonts() {
    let dir = assets_dir().join("fonts");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        eprintln!("no font directory {}", dir.display());
        return;
    };
    let mut fonts: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("ttf")))
        .collect();
    fonts.sort();
    for font in fonts {
        if ffi::app_add_font(&qpath(&font)) < 0 {
            let name = font.file_name().unwrap_or_default().to_string_lossy();
            eprintln!("could not load font {name}");
        }
    }
}

/// "materialfy: <module>: <message>".
struct LogFormat;

impl<S, N> tracing_subscriber::fmt::FormatEvent<S, N> for LogFormat
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
    N: for<'a> tracing_subscriber::fmt::FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &tracing_subscriber::fmt::FmtContext<'_, S, N>,
        mut writer: tracing_subscriber::fmt::format::Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> std::fmt::Result {
        write!(writer, "materialfy: {}: ", event.metadata().target())?;
        ctx.field_format().format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

fn init_tracing() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .event_format(LogFormat)
        .try_init();
}

fn build_runtime() -> std::io::Result<tokio::runtime::Runtime> {
    // librespot (Session/Spirc), the player actor, Web API polling and bridge tasks share this runtime
    let workers = std::thread::available_parallelism().map_or(3, |n| n.get()).clamp(3, 6);
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(workers)
        .thread_name("materialfy-rt")
        .enable_all()
        .build()
}

fn initial_value(value: &InitialValue) -> QVariant {
    match value {
        InitialValue::Bool(b) => QVariant::from(b),
        InitialValue::Int(i) => match i32::try_from(*i) {
            Ok(small) => QVariant::from(&small),
            Err(_) => QVariant::from(i),
        },
        InitialValue::Float(f) => QVariant::from(f),
        InitialValue::Str(s) => QVariant::from(&qstr(s)),
    }
}

fn initial_properties(cli: &Cli) -> QVariantMap {
    let mut map = QVariantMap::default();
    if let Some((width, height)) = cli.size {
        map.insert(qstr("width"), QVariant::from(&width));
        map.insert(qstr("height"), QVariant::from(&height));
    }
    for (key, value) in &cli.set {
        map.insert(qstr(key), initial_value(value));
    }
    map
}

/// MPRIS Raise/Quit -> the GUI thread (C++ moves the call into it itself).
/// Windows/macOS: "show the window" requests from new processes (on Linux they arrive via MPRIS Raise).
fn spawn_instance_listener(rt: &tokio::runtime::Handle) {
    rt.spawn(async move {
        let mut listener = match mf_platform::single_instance::InstanceListener::start().await {
            Ok(listener) => listener,
            Err(e) => {
                tracing::debug!("single-instance listener not started: {e}");
                return;
            }
        };
        while listener.recv().await.is_some() {
            ffi::post_raise_window();
        }
    });
}

fn spawn_app_requests(rt: &tokio::runtime::Handle, services: &Services) {
    let mut requests = services.app_requests();
    rt.spawn(async move {
        loop {
            match requests.recv().await {
                Ok(AppRequest::RaiseWindow) => ffi::post_raise_window(),
                Ok(AppRequest::Quit) => ffi::post_quit(),
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
        }
    });
}

/// The process exit code.
pub fn run(cli: Cli) -> i32 {
    setup_env(&cli);
    let qml_guard = crate::qml_cache::prepare(cli.screenshot.is_none());
    let config_dir = cli.config_dir.clone().or_else(|| cli.screenshot.as_ref().map(|_| make_temp_dir()));
    let settings = SettingsStore::open(config_dir);
    apply_gpu_preference(&settings);
    apply_ui_scale(&settings);

    ffi::app_set_names(&qstr("Materialfy"), &qstr("materialfy"), &qstr("materialfy"));
    let args: Vec<String> = std::env::args_os().map(|a| a.to_string_lossy().into_owned()).collect();
    ffi::app_create(&args);

    let rt = match build_runtime() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("materialfy: could not start tokio: {e}");
            return 1;
        }
    };

    // The player is already running (possibly hidden in the tray or still starting) - we show its window instead of a second
    // instance. --new-instance - for development and screenshots
    if cli.main_mode() && !cli.new_instance && !rt.block_on(mf_platform::single_instance::become_primary()) {
        return 0;
    }
    if let Some(guard) = qml_guard {
        guard.arm(rt.handle());
    }

    // window icon; on Windows shortcuts and the taskbar also take the IDI_ICON1 resource from the exe (build.rs)
    let icon = assets_dir().join("materialfy.svg");
    if icon.is_file() {
        ffi::app_set_window_icon(&qpath(&icon));
    }
    // Before windows are created: Qt Quick takes the frame period from the primary screen (see display.rs)
    let screen_note = crate::display::prefer_fastest_screen();
    if cli.debug_fps {
        eprintln!("{screen_note}");
    }
    load_fonts();
    init_tracing();

    // UI language before any service or QML produces text (default: the OS UI language — Russian or English)
    crate::bridges::translations::apply_language(crate::bridges::translations::effective_language(&settings));
    let options = ServicesOptions {
        // Screenshots - without a session, except for an explicit file for tests
        persist_session: cli.screenshot.is_none() || env_set("MATERIALFY_SESSION_FILE"),
        enable_media: cli.screenshot.is_none(),
    };
    let services = match Services::start(rt.handle(), settings, options) {
        Ok(services) => services,
        Err(e) => {
            eprintln!("materialfy: {e:#}");
            return 1;
        }
    };

    context::init(AppContext {
        rt: rt.handle().clone(),
        services: services.clone(),
        flags: StartupFlags {
            screenshot: cli.screenshot.is_some(),
            light: cli.light,
            seed_image: cli.seed_image.as_deref().map(absolute),
        },
        system_dark: crate::qt::system_dark_watch(),
    });
    crate::qt::ffi::ffi::start_gui_heartbeat();
    // All singletons - before the engine is created: registering after the module loads breaks type resolution
    ffi::register_qml_types();

    let code = run_engine(&cli, &rt, &services);
    services.shutdown();
    rt.shutdown_timeout(Duration::from_secs(1));
    ffi::app_destroy();
    code
}

fn run_engine(cli: &Cli, rt: &tokio::runtime::Runtime, services: &Services) -> i32 {
    let mut engine = QQmlApplicationEngine::new();
    let Some(mut e) = engine.as_mut() else {
        eprintln!("materialfy: could not create QQmlApplicationEngine");
        return 1;
    };
    crate::images::install_caching_nam(e.as_mut(), rt.handle(), services);
    e.as_mut().add_import_path(&qpath(&qml_dir()));
    ffi::engine_set_context_bool(e.as_mut(), &qstr("materialfyDebugWheel"), cli.debug_wheel);
    ffi::engine_set_context_bool(e.as_mut(), &qstr("materialfyDebugFps"), cli.debug_fps);
    // Tray icon: not for screenshots and not for the gallery
    ffi::engine_set_context_bool(e.as_mut(), &qstr("materialfyTray"), cli.screenshot.is_none() && !cli.gallery);

    let entry = match &cli.qml {
        Some(file) => absolute(file),
        None => qml_dir().join(if cli.gallery { "gallery/Gallery.qml" } else { "Main.qml" }),
    };
    e.as_mut().set_initial_properties(&initial_properties(cli));
    e.as_mut().load(&QUrl::from_local_file(&qpath(&entry)));

    let root = ffi::engine_root_object(&e);
    if root.is_null() {
        let msg = format!("QQmlApplicationEngine failed to load root component from {}\n", entry.display());
        let path = mf_platform::dirs::app_data_dir().join("crash.log");
        let _ = std::fs::create_dir_all(mf_platform::dirs::app_data_dir());
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            use std::io::Write as _;
            let _ = f.write_all(msg.as_bytes());
        }
        eprintln!("{msg}");
        return 1;
    }
    if !cli.offline {
        services.auth.start();
    }

    // SAFETY: root is a live root object of the engine, everything in the GUI thread
    unsafe {
        if ffi::object_has_property(root, &qstr("trayWanted")) {
            // The main window may hide into the tray - the exit is decided by it itself (Main.qml, onClosing), not by Qt
            ffi::app_set_quit_on_last_window_closed(false);
        }
        ffi::set_main_window(root);
    }
    // SAFETY: root is a live window of the engine, GUI thread
    unsafe { ffi::watch_software_rendering(root) };
    if cli.screenshot.is_none() {
        crate::winframe::install(root);
        // SAFETY: root is a live window of the engine, GUI thread
        unsafe { ffi::install_memory_trim(root) };
    }
    spawn_app_requests(rt.handle(), services);
    if cli.main_mode() && cfg!(any(target_os = "windows", target_os = "macos")) {
        spawn_instance_listener(rt.handle());
    }

    if let Some(path) = &cli.screenshot {
        let delay = i32::try_from(cli.delay).unwrap_or(i32::MAX);
        // SAFETY: root is alive while the engine is alive; the timer holds a QPointer
        unsafe { ffi::schedule_screenshot(root, &qpath(path), delay) };
    }

    let code = ffi::app_exec();
    drop(engine);
    code
}

fn find_qml_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("MATERIALFY_QML_DIR").filter(|v| !v.is_empty()) {
        return absolute(Path::new(&dir));
    }
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf));
    if let Some(exe_dir) = &exe_dir {
        let packaged = exe_dir.join("qml");
        if packaged.join("Main.qml").is_file() {
            return packaged;
        }
    }
    let cwd = std::env::current_dir().ok();
    for start in exe_dir.iter().chain(cwd.iter()) {
        for dir in start.ancestors() {
            let candidate = dir.join("src").join("materialfy").join("qml");
            if candidate.join("Main.qml").is_file() {
                return candidate;
            }
        }
    }
    absolute(Path::new("src/materialfy/qml"))
}

/// The src/materialfy/qml directory (see the header).
pub fn qml_dir() -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(find_qml_dir).clone()
}

/// The src/materialfy/assets directory.
pub fn assets_dir() -> PathBuf {
    let qml = qml_dir();
    qml.parent().map_or_else(|| PathBuf::from("assets"), |p| p.join("assets"))
}
