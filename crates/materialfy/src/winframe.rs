//! Frameless window on Windows: native resize, Aero Snap, shadow and rounded Windows 11 corners via DWM
//! (cpp/winframe.cpp). See contract §7. On other OSes - nothing: resize is done by QML ResizeEdges via
//! startSystemResize, corners are rounded by QML WindowFrame (the window is transparent).
//! There too - the "Back / Pause / Next" buttons under the window thumbnail on the Windows taskbar (cpp/taskbar.cpp)
//! and the QML singleton WindowShell (minimize the window without losing "maximized" / "fullscreen"; player windows outside the main
//! window - the island and the taskbar widget, cpp/floating.cpp).

use cxx_qt_lib::QString;

/// Resize zone width, logical pixels (like ResizeEdges.grip).
pub const GRIP: i32 = 6;

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    #[namespace = "mf"]
    unsafe extern "C++" {
        include!("winframe.h");
        /// setWindowStates(windowStates() | Minimized) - see WindowShell.minimize
        unsafe fn window_minimize(window: *mut QObject);
        include!("taskbar.h");
        unsafe fn taskbar_install(window: *mut QObject) -> bool;
        include!("floating.h");
        unsafe fn floating_tool_window(window: *mut QObject) -> bool;
        unsafe fn floating_set_mask(window: *mut QObject, x: i32, y: i32, w: i32, h: i32);
        unsafe fn floating_set_mask_rounded(window: *mut QObject, x: i32, y: i32, w: i32, h: i32, radius: i32);
        unsafe fn floating_fullscreen_on(window: *mut QObject) -> bool;
        unsafe fn floating_maximized_on(window: *mut QObject) -> bool;
        unsafe fn floating_cursor_in(window: *mut QObject, x: i32, y: i32, w: i32, h: i32) -> i32;
        unsafe fn floating_taskbar_place(window: *mut QObject, width: i32, height: i32, embed: bool, pos: &QString) -> i32;
        unsafe fn floating_taskbar_drag(window: *mut QObject, width: i32, height: i32, phase: i32) -> QString;
        unsafe fn floating_taskbar_detach(window: *mut QObject);
        unsafe fn floating_key_hook(window: *mut QObject, on: bool) -> bool;
        unsafe fn floating_set_click_through(window: *mut QObject, enable: bool);
        fn floating_windhawk_active() -> bool;
    }

    unsafe extern "RustQt" {
        /// For the QML window: Window.showMinimized() and visibility = Window.Minimized reset "maximized" and
        /// "fullscreen" (Qt sets the state to exactly Minimized), and the window comes back from the taskbar as a normal one.
        /// WindowShell.minimize(window) adds Minimized to the current state - the OS remembers which one to restore.
        #[qobject]
        type WindowShell = super::WindowShellRust;

        #[qinvokable]
        unsafe fn minimize(self: &WindowShell, window: *mut QObject);

        /// Show and raise the main window (from the tray, minimized too), like a second launch of the application
        #[qinvokable]
        #[cxx_name = "raiseMain"]
        fn raise_main(self: &WindowShell);
        /// Island window: not in the taskbar or Alt+Tab, does not take focus (Windows; otherwise false)
        #[qinvokable]
        #[cxx_name = "makeToolWindow"]
        unsafe fn make_tool_window(self: &WindowShell, window: *mut QObject) -> bool;
        /// KDE Plasma session on Wayland: the island can be placed by a KWin script (kwinPlace)
        #[qinvokable]
        #[cxx_name = "kwinAvailable"]
        fn kwin_available(self: &WindowShell) -> bool;
        /// KDE Wayland: the KWin script places the window with this title at (x, y, w, h) logical px of the desktop,
        /// above everything and outside the taskbar, and hides it under a fullscreen / maximized window on the same monitor.
        /// Dragged by the mouse (startSystemMove) - the window moves only along the move_x / move_y axes, the end is the kwinMoved signal
        #[qinvokable]
        #[cxx_name = "kwinPlace"]
        fn kwin_place(
            self: &WindowShell,
            title: &QString,
            x: i32,
            y: i32,
            w: i32,
            h: i32,
            hide_fullscreen: bool,
            hide_maximized: bool,
            move_x: bool,
            move_y: bool,
        );
        /// KDE Wayland: the window with this title was dragged by the mouse and released at (x, y) logical px of the desktop
        #[qsignal]
        #[cxx_name = "kwinMoved"]
        fn kwin_moved(self: Pin<&mut WindowShell>, title: QString, x: i32, y: i32);
        /// Unload this window's KWin script
        #[qinvokable]
        #[cxx_name = "kwinRelease"]
        fn kwin_release(self: &WindowShell, title: &QString);
        /// The mouse is captured and only a rectangle is drawn (logical px of the window); w <= 0 - the whole window
        #[qinvokable]
        #[cxx_name = "setMask"]
        unsafe fn set_mask(self: &WindowShell, window: *mut QObject, x: i32, y: i32, w: i32, h: i32);
        /// The mouse is captured and only a rounded rectangle is drawn (logical px of the window); w <= 0 - the whole window
        #[qinvokable]
        #[cxx_name = "setMaskRounded"]
        unsafe fn set_mask_rounded(self: &WindowShell, window: *mut QObject, x: i32, y: i32, w: i32, h: i32, radius: i32);
        /// A fullscreen application (video, game, browser via F11, presentation) is on the window's monitor - Windows
        #[qinvokable]
        #[cxx_name = "fullscreenOn"]
        unsafe fn fullscreen_on(self: &WindowShell, window: *mut QObject) -> bool;
        /// A maximized window is on the window's monitor - Windows
        #[qinvokable]
        #[cxx_name = "maximizedOn"]
        unsafe fn maximized_on(self: &WindowShell, window: *mut QObject) -> bool;
        /// The cursor is over the window rectangle (logical px), even if the window is transparent to the mouse:
        /// 0 - no, 1 - yes, 2 - yes, but a mouse button is held
        #[qinvokable]
        #[cxx_name = "cursorIn"]
        unsafe fn cursor_in(self: &WindowShell, window: *mut QObject, x: i32, y: i32, w: i32, h: i32) -> i32;
        /// Embed the window in the Windows taskbar and put it at position pos (taskbarWidgetPos): 1 - embedded,
        /// 2 - above the bar, 0 - nowhere to put it, -1 - the window is lost (Explorer restart), create a new one. See cpp/floating.h
        #[qinvokable]
        #[cxx_name = "taskbarPlace"]
        unsafe fn taskbar_place(self: &WindowShell, window: *mut QObject, width: i32, height: i32, embed: bool, pos: &QString) -> i32;
        /// Dragging the widget along the bar following the cursor (phase 0 - start, 1 - step, 2 - end) -> position
        #[qinvokable]
        #[cxx_name = "taskbarDrag"]
        unsafe fn taskbar_drag(self: &WindowShell, window: *mut QObject, width: i32, height: i32, phase: i32) -> QString;
        #[qinvokable]
        #[cxx_name = "taskbarDetach"]
        unsafe fn taskbar_detach(self: &WindowShell, window: *mut QObject);
        /// Arrows and space without focus while the cursor is over the window (Windows): the window's floatingKey(code) QML function
        #[qinvokable]
        #[cxx_name = "keyHook"]
        unsafe fn key_hook(self: &WindowShell, window: *mut QObject, on: bool) -> bool;
        /// Pass clicks through the window: true - clicks go to the windows under it
        #[qinvokable]
        #[cxx_name = "setClickThrough"]
        unsafe fn set_click_through(self: &WindowShell, window: *mut QObject, enable: bool);
        /// Windhawk is running with a taskbar style: the widget is then a window above the bar, not inside it
        #[qinvokable]
        #[cxx_name = "windhawkActive"]
        fn windhawk_active(self: &WindowShell) -> bool;
    }

    impl cxx_qt::Threading for WindowShell {}
}

#[derive(Default)]
pub struct WindowShellRust;

impl qobject::WindowShell {
    unsafe fn minimize(&self, window: *mut cxx_qt::QObject) {
        if !window.is_null() {
            // SAFETY: the window comes from QML (a live QObject), called from the GUI thread
            unsafe { qobject::window_minimize(window) };
        }
    }

    fn raise_main(&self) {
        crate::qt::ffi::ffi::post_raise_window();
    }

    fn kwin_available(&self) -> bool {
        mf_platform::kwin::available()
    }

    #[allow(clippy::too_many_arguments)]
    fn kwin_place(&self, title: &QString, x: i32, y: i32, w: i32, h: i32, hide_fullscreen: bool, hide_maximized: bool, move_x: bool, move_y: bool) {
        static REPORTS: std::sync::Once = std::sync::Once::new();
        REPORTS.call_once(|| {
            let thread = cxx_qt::Threading::qt_thread(self);
            mf_platform::kwin::on_moved(Box::new(move |title, x, y| {
                let _ = thread.queue(move |shell| shell.kwin_moved(QString::from(title.as_str()), x, y));
            }));
        });
        let title = title.to_string();
        let spot = mf_platform::kwin::Spot { x, y, width: w, height: h, hide_fullscreen, hide_maximized, move_x, move_y };
        crate::context::ctx().rt.spawn(async move {
            if let Err(error) = mf_platform::kwin::place(&title, spot).await {
                tracing::warn!("KWin script for {title}: {error}");
            }
        });
    }

    fn kwin_release(&self, title: &QString) {
        let title = title.to_string();
        // on exit the background task will not have time: we wait for the script to unload (not long), otherwise it stays in KWin until restart
        crate::context::ctx().rt.block_on(async move {
            let _ = tokio::time::timeout(std::time::Duration::from_millis(500), mf_platform::kwin::release(&title)).await;
        });
    }

    unsafe fn make_tool_window(&self, window: *mut cxx_qt::QObject) -> bool {
        // SAFETY: the window comes from QML (a live QObject), called from the GUI thread
        !window.is_null() && unsafe { qobject::floating_tool_window(window) }
    }

    unsafe fn set_mask(&self, window: *mut cxx_qt::QObject, x: i32, y: i32, w: i32, h: i32) {
        if !window.is_null() {
            // SAFETY: same
            unsafe { qobject::floating_set_mask(window, x, y, w, h) };
        }
    }

    unsafe fn set_mask_rounded(&self, window: *mut cxx_qt::QObject, x: i32, y: i32, w: i32, h: i32, radius: i32) {
        if !window.is_null() {
            // SAFETY: same
            unsafe { qobject::floating_set_mask_rounded(window, x, y, w, h, radius) };
        }
    }

    unsafe fn maximized_on(&self, window: *mut cxx_qt::QObject) -> bool {
        // SAFETY: same
        !window.is_null() && unsafe { qobject::floating_maximized_on(window) }
    }

    unsafe fn fullscreen_on(&self, window: *mut cxx_qt::QObject) -> bool {
        // SAFETY: same
        !window.is_null() && unsafe { qobject::floating_fullscreen_on(window) }
    }

    unsafe fn cursor_in(&self, window: *mut cxx_qt::QObject, x: i32, y: i32, w: i32, h: i32) -> i32 {
        if window.is_null() {
            return 0;
        }
        // SAFETY: same
        unsafe { qobject::floating_cursor_in(window, x, y, w, h) }
    }

    unsafe fn taskbar_place(&self, window: *mut cxx_qt::QObject, width: i32, height: i32, embed: bool, pos: &QString) -> i32 {
        if window.is_null() {
            return 0;
        }
        // SAFETY: same
        unsafe { qobject::floating_taskbar_place(window, width, height, embed, pos) }
    }

    unsafe fn taskbar_drag(&self, window: *mut cxx_qt::QObject, width: i32, height: i32, phase: i32) -> QString {
        if window.is_null() {
            return QString::default();
        }
        // SAFETY: same
        unsafe { qobject::floating_taskbar_drag(window, width, height, phase) }
    }

    unsafe fn key_hook(&self, window: *mut cxx_qt::QObject, on: bool) -> bool {
        // SAFETY: same; removing a hook with a null window - remove any
        unsafe { qobject::floating_key_hook(window, on) }
    }

    unsafe fn taskbar_detach(&self, window: *mut cxx_qt::QObject) {
        if !window.is_null() {
            // SAFETY: same
            unsafe { qobject::floating_taskbar_detach(window) };
        }
    }

    unsafe fn set_click_through(&self, window: *mut cxx_qt::QObject, enable: bool) {
        if !window.is_null() {
            // SAFETY: same
            unsafe { qobject::floating_set_click_through(window, enable) };
        }
    }

    fn windhawk_active(&self) -> bool {
        qobject::floating_windhawk_active()
    }
}

/// Install a filter on the main window (after load, the window already exists). `window` is the root QQuickWindow*.
pub fn install(window: *mut cxx_qt::QObject) {
    if !cfg!(target_os = "windows") || window.is_null() {
        return;
    }
    // SAFETY: window is a live root object of the engine, called from the GUI thread
    if !unsafe { crate::qt::ffi::ffi::winframe_install(window, GRIP) } {
        tracing::warn!("window frame: root is not a QWindow or has no HWND — resizing via QML");
    }
    // SAFETY: the same window; Player and the other singletons are already created (register_qml_types before load)
    if !unsafe { qobject::taskbar_install(window) } {
        tracing::warn!("taskbar: no thumbnail toolbar (no HWND, QML engine or Player)");
    }
}
