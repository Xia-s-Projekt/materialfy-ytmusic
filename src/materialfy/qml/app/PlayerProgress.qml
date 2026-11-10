import QtQuick
import Md3
import Materialfy.Core

// The played fraction for the seek bar, smooth between position updates (they arrive ~5 times a second,
// by polling for another device): advanced off the shared Ticker clock (~30 Hz), in the same frames as the running
// wave. This used to be a Behavior animation on every update: it ran almost continuously at monitor refresh
// and repainted the whole window while music played, even for a hidden bar.
QtObject {
    id: root

    property bool active: true             // the bar is visible and is not being dragged
    property real value: Player.position

    readonly property Connections _sync: Connections {
        target: Player
        function onPositionChanged() { root.value = Player.position }
    }

    readonly property TickAnimation _tick: TickAnimation {
        running: root.active && Player.playing && Player.durationMs > 0 && !Theme.quiet
        onTriggered: root.value = Math.min(1, root.value + root._tick.frameTime * 1000 / Player.durationMs)
    }
}
