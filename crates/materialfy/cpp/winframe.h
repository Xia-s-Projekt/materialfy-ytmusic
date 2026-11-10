#pragma once

#include <QtCore/QObject>

namespace mf {

// A frameless window on Windows: native resize by the edges, Aero Snap, a DWM shadow (contract §7).
// window — the root's QWindow* (after load). grip — the width of the resize area in logical pixels (6).
// false — not a QWindow, or there is no HWND.
bool winframe_install(QObject* window, int grip);

// Minimize while keeping "maximized" / "fullscreen": setWindowStates(windowStates() | Qt::WindowMinimized).
// QWindow::showMinimized() sets exactly Minimized — Windows and X11 then restore the window as a normal one (issue #2).
void window_minimize(QObject* window);

} // namespace mf
