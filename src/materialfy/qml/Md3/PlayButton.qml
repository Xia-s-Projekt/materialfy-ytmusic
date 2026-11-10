import QtQuick

// Play/Pause in an MD3 Expressive "cookie". During playback the shape slowly rotates,
// on press it squeezes into a rounder one, and morphs when the state changes.
Item {
    id: root

    property bool playing: false
    property real size: 64
    property color containerColor: Theme.primary
    property color iconColor: Theme.fgPrimary
    property var playingShape: "softSquare"
    property var pausedShape: "softSquare"
    property bool spin: true
    // the selected track is still loading: an MD3 indicator instead of the icon, the button stays clickable
    property bool loading: false

    signal clicked()

    implicitWidth: size
    implicitHeight: size

    MorphShape {
        id: shape
        anchors.fill: parent
        anchors.margins: area.pressed ? root.size * 0.06 : 0
        Behavior on anchors.margins { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutBack } }
        shape: area.pressed ? "circle" : root.playing ? root.playingShape : root.pausedShape
        duration: Theme.motion.spatialDefault
        // disabled (nothing is playing) — a dimmed MD3 container
        color: root.enabled ? root.containerColor : Qt.alpha(Theme.fgSurface, Theme.stateLayer.disabledContainer)

        TickAnimation {
            running: root.spin && root.playing && root.visible && !Theme.quiet && !Theme.reducedMotion
            // rotate the item, not the outline (angle) — no path rebuild every frame
            onTriggered: shape.rotation = (shape.rotation + frameTime * 30) % 360
        }
    }

    Rectangle {
        anchors.fill: parent
        radius: width / 2
        color: root.iconColor
        opacity: area.containsMouse && !area.pressed ? Theme.stateLayer.hover : 0
        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
    }

    Icon {
        anchors.centerIn: parent
        visible: !root.loading
        name: root.playing ? "pause" : "play_arrow"
        size: root.size * 0.45
        fill: 1
        color: root.enabled ? root.iconColor : Qt.alpha(Theme.fgSurface, Theme.stateLayer.disabledContent)
    }

    LoadingIndicator {
        anchors.centerIn: parent
        size: root.size * 0.5
        color: root.enabled ? root.iconColor : Theme.fgSurface
        visible: root.loading
        running: root.loading && root.visible
    }

    MouseArea {
        id: area
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: root.enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
        onClicked: root.clicked()
    }
}
