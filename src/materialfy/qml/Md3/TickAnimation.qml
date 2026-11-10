import QtQuick

// A FrameAnimation replacement for decorative infinite animations: it ticks off the shared Ticker clock (~30 Hz).
// The same interface: running, frameTime, onTriggered.
QtObject {
    id: root

    property bool running: false
    property real frameTime: 0
    signal triggered()

    property bool _held: false
    function _sync() {
        if (running === _held)
            return
        _held = running
        Ticker.users += running ? 1 : -1
    }
    onRunningChanged: _sync()
    Component.onCompleted: _sync()
    Component.onDestruction: if (_held) { _held = false; Ticker.users -= 1 }

    property Connections _c: Connections {
        target: Ticker
        enabled: root.running
        function onTick(dt) {
            root.frameTime = dt
            root.triggered()
        }
    }
}
