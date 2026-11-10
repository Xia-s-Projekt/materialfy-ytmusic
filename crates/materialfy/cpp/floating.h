#pragma once

#include <QtCore/QObject>
#include <QtCore/QString>

namespace mf {

// The player outside the main window (the floatingPlayer setting): an island at the top edge of the screen and a widget in the taskbar.
// window — a QWindow* (Window from QML). Outside Windows — stubs (false / 0 / ""), except for the masks and floating_cursor_in.

// The island window: not in the taskbar or Alt+Tab, does not take focus (WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE),
// DWM does not round the corners. macOS (floating_mac.mm): above the menu bar, on all desktops, the screen notch size —
// in the window properties notchW / notchH. false — a different OS, or there is no window.
bool floating_tool_window(QObject* window);

// Where the window catches the mouse and is drawn: a rectangle in the window's logical pixels; w <= 0 — the whole window.
// The rest (transparent) part of the window lets clicks through to the windows below it (Windows — a window region, X11 — XShape).
// radius > 0 — with rounded corners; the part of the shape above y = 0 cuts off the window's edge (the island: a flat top).
void floating_set_mask(QObject* window, int x, int y, int w, int h);
void floating_set_mask_rounded(QObject* window, int x, int y, int w, int h, int radius);

// A fullscreen application on the window's monitor (video, a game, a browser in F11, a presentation): the player has no business on top of it.
// It looks at the foreground window on that very monitor — a game on a second monitor does not affect the first.
bool floating_fullscreen_on(QObject* window);

// A maximized window on the window's monitor (the "maximize" button): the island hides under it too, per the setting. Windows —
// the foreground window or whatever is visible at the monitor's center; on KDE Wayland a KWin script does this (kwin.rs), outside them — false.
bool floating_maximized_on(QObject* window);

// The cursor over the rectangle (the window's logical px; the window may also be transparent to the mouse): 0 — no,
// 1 — yes, 2 — yes, but a mouse button is held down (the window is being dragged or a selection is being made — the island does not pop up).
int floating_cursor_in(QObject* window, int x, int y, int w, int h);

// The widget in the Windows taskbar: embed the window as a child of Shell_TrayWnd and put it in place.
// width × height — the size in logical pixels (the window's scale is its devicePixelRatio, that is the taskbar's DPI ×
// the interface scale; the height is no more than the taskbar without padding, and the limit is computed in the taskbar's pixels),
// embed: false — go straight to the fallback. pos — the place (the taskbarWidgetPos setting): "tray" — by the notification area,
// "left" — at the left edge (after the Start button and search, if the icons are on the left), "appsLeft" / "appsRight" — left / right
// of the application icons, or a 0..1 fraction of the taskbar's free width ("0.42").
// Returns 1 — embedded, 2 — the fallback (a window above the taskbar, owned by it), 0 — nowhere to put it (no taskbar, it is
// vertical, narrow, or currently hidden), -1 — the window is lost (Explorer restarted and took the child window
// with it) — it has to be created again. Called once every 2 s: it also catches taskbar size and DPI changes.
int floating_taskbar_place(QObject* window, int width, int height, bool embed, const QString& pos);

// Dragging the widget along the taskbar with the cursor: phase 0 — the start (remembers where it was grabbed), 1 — a step,
// 2 — the end. Within 24 px the window snaps to the "left", "appsLeft", "appsRight", "tray" places. Returns the place
// in the taskbarWidgetPos format ("" — it did not work out).
QString floating_taskbar_drag(QObject* window, int width, int height, int phase);

// Take the window out of the taskbar (before deleting the window)
void floating_taskbar_detach(QObject* window);

// Keys without focus: while the cursor is over the window, ← → ↑ ↓ and space (without Ctrl, Alt, Win) do not reach the active window,
// but call the window's floatingKey(code) QML function instead: 1 ←, 2 →, 3 ↑, 4 ↓, 5 space. Windows — a low-level
// keyboard hook (WH_KEYBOARD_LL) only while hovering: on — install, false — remove. The cursor left the window
// or has been still for more than 6 s (the mouse was left at the edge and someone is typing) — the keys go through as usual again.
// Outside Windows — false (we do not take focus).
bool floating_key_hook(QObject* window, bool on);

// Letting clicks through the window (WS_EX_TRANSPARENT): true — clicks go to the windows below it
void floating_set_click_through(QObject* window, bool enable);

// Windhawk is running with a taskbar style (windows-11-taskbar-styler): the taskbar's XAML overlaps child windows,
// the widget then becomes a window above the taskbar
bool floating_windhawk_active();

} // namespace mf
