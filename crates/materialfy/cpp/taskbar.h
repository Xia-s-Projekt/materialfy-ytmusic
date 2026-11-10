#pragma once

#include <QtCore/QObject>

namespace mf {

// Windows: "Previous / Pause-Play / Next" buttons under the window thumbnail on the taskbar
// (ITaskbarList3::ThumbBarAddButtons), like Spotify's. window — the root's QWindow* (after load, the singletons
// of Materialfy.Core already exist). false — not Windows, or there is no HWND, no QML engine, or no Player.
bool taskbar_install(QObject* window);

} // namespace mf
