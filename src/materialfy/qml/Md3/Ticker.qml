pragma Singleton
import QtQuick

// Shared "clock" for decorative infinite animations (background drift, running wave, cover rotation, "breathing"
// of the blobs): ~30 Hz instead of the monitor rate, and they all fire in one frame. Each such animation on
// a FrameAnimation forced the whole window to redraw at 144-280 Hz while music played.
// Subscribers are TickAnimation; the timer only runs while at least one of them is active.
QtObject {
    id: root

    property int users: 0
    property int interval: 33
    signal tick(real dt)

    property real _last: 0
    property Timer _timer: Timer {
        interval: root.interval
        repeat: true
        running: root.users > 0
        onRunningChanged: root._last = Date.now()
        onTriggered: {
            const now = Date.now()
            // after a pause (minimized window, sleep) — do not jump by the whole missed interval
            const dt = Math.min(0.1, Math.max(0, (now - root._last) / 1000))
            root._last = now
            root.tick(dt)
        }
    }
}
