// Small Qt helpers that are missing from cxx-qt-lib. Owner — the "shell" agent.
#pragma once

#include <QtCore/QObject>
#include <QtCore/QRectF>
#include <QtCore/QString>
#include <QtCore/QStringList>
#include <QtGui/QColor>
#include <QtQml/QQmlApplicationEngine>

#include <cstdint>

#include "rust/cxx.h"

namespace mf {

// QApplication (Qt Widgets: Qt.labs.platform takes the tray and menus from it on every OS).
// The arguments are copied and live until the process ends.
void app_create(rust::Slice<const rust::String> args);
int app_exec();
// Delete the QApplication at the end of run (after the engine and after the background threads stop)
void app_destroy();
void app_quit();
void app_set_names(const QString& app, const QString& org, const QString& desktop);
// An environment variable that Qt also sees (on Windows it reads the C runtime copy, not the process environment)
void set_qt_env(const QString& name, const QString& value);
// Windows and Linux: a crash in native code (Qt, a driver): the code or signal and the call chain go to crash.log
void install_native_crash_log(const QString& path);
void app_set_quit_on_last_window_closed(bool value);
void app_set_window_icon(const QString& path);
// QFontDatabase::addApplicationFont; -1 — an error
int app_add_font(const QString& path);
// Qt's own strings (dialogs, context menus): qtbase_<code>.qm from translations/ next to the exe or Qt's
// TranslationsPath; "en" removes the translator. Also sets the default QLocale.
void app_set_qt_language(const QString& code);
// The OS UI languages, most preferred first (QLocale::system().uiLanguages(), BCP 47: "ru-RU", "en-US"…).
// Not affected by app_set_qt_language (QLocale::setDefault).
QStringList system_ui_languages();

// The clipboard (Auth.copyToClipboard, Catalog.copyLink)
void copy_to_clipboard(const QString& text);

// An object handed to QML from a slot, otherwise it becomes JavaScriptOwnership and is collected by the garbage collector
void set_cpp_ownership(QObject* object);

// System theme: true — dark (QStyleHints::colorScheme() != Light)
bool system_color_scheme_dark();
// Subscription to QStyleHints::colorSchemeChanged → Rust system_dark_changed(bool)
void watch_color_scheme();
// The GUI thread is alive: gui_heartbeat() once a second (single_instance: a hung window is not "shown")
void start_gui_heartbeat();
// A folder in the file manager (Settings.openCacheFolder)
bool open_local_folder(const QString& path);

// QML loading: the engine, context properties, the root's initial properties
void engine_add_import_path(QQmlApplicationEngine& engine, const QString& path);
void engine_set_context_bool(QQmlApplicationEngine& engine, const QString& name, bool value);
// Create all module singletons up front (like qmlRegisterSingletonInstance in the Python version)
void engine_touch_singletons(QQmlApplicationEngine& engine, const QStringList& names);
bool engine_has_root(const QQmlApplicationEngine& engine);
// The first root object (QQuickWindow) or nullptr
QObject* engine_root_object(const QQmlApplicationEngine& engine);
// The on-disk image cache (images.py): each NAM has its own QNetworkDiskCache on a shared folder, HTTP/1.1 only
void engine_install_caching_nam(QQmlApplicationEngine& engine, const QString& dir, std::int64_t max_bytes);
// A proxy for these NAMs (thread-safe, applies to subsequent requests):
// 0 — no proxy, 1 — system, 2 — HTTP, 3 — SOCKS5, 4 — blocked (invalid settings)
void set_image_proxy(int mode, const QString& host, int port, const QString& user, const QString& password);

// Whether an object has a property (property(name).isValid(), like `is not None` in app.py)
bool object_has_property(QObject* object, const QString& name);
// The window that AppRequest::RaiseWindow shows (held through a QPointer)
void set_main_window(QObject* window);
// Thread-safe: run show/raise/requestActivate on the main window, or exit, in the GUI thread
void post_raise_window();
void post_quit();
// After delay ms: grabWindow().save(path) and exit
void schedule_screenshot(QObject* window, const QString& path, int delay_ms);
// Hidden to the tray or minimized for a few seconds: drop the scene graph and the graphics device
// (QQuickWindow::releaseResources with non-persistent graphics), collect JS garbage, compact the heaps and
// trim the working set. Showing the window rebuilds everything on the first frame.
void install_memory_trim(QObject* window);
// Software rendering (llvmpipe, WARP, SwiftShader, Qt Quick software backend: the CPU draws every frame): once the
// scene graph is up, the window gets softwareRendering = true and drops its decorative effects.
// MATERIALFY_SOFTWARE_RENDERING=1 / 0 forces it either way.
void watch_software_rendering(QObject* window);

// The fastest monitor becomes the primary one (display.py). A string for the log.
QString prefer_fastest_screen();

// IconMetrics.ink: the offset of the glyph outline's center and its size (icons.py, _measure)
QRectF icon_measure(const QString& text, int size);

} // namespace mf
