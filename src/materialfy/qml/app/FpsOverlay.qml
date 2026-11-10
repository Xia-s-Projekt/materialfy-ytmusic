import QtQuick
import Md3

// --debug-fps: the window's frame rate (smoothed) and the worst frame per second.
// The window repaints only on changes, so we measure during scrolling/animations.
Rectangle {
    id: root

    property real fps: 0
    property real worst: 0
    property int _frames: 0
    property real _acc: 0
    property real _worstAcc: 0

    width: label.implicitWidth + 16
    height: 28
    radius: 14
    color: Qt.rgba(0, 0, 0, 0.7)
    z: 10000

    FrameAnimation {
        running: true   // it keeps the repaint going itself — shows the ceiling the compositor gives
        onTriggered: {
            root._frames++
            root._acc += frameTime
            root._worstAcc = Math.max(root._worstAcc, frameTime)
            if (root._acc >= 1) {
                root.fps = root._frames / root._acc
                root.worst = root._worstAcc * 1000
                root._frames = 0; root._acc = 0; root._worstAcc = 0
            }
        }
    }

    Text {
        id: label
        anchors.centerIn: parent
        color: "white"
        font.pixelSize: 13
        font.family: "monospace"
        text: I18n.t("{0} fps · worst {1} ms", root.fps.toFixed(0), root.worst.toFixed(1))
    }
}
