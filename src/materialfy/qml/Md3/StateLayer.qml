import QtQuick
import QtQuick.Effects

// MD3 interactive surface: hover/press layer + ripple, clipped to the corner radius.
// It is placed over the component's content: StateLayer { anchors.fill: parent; radius: ... }
MouseArea {
    id: root

    property color color: Theme.fgSurface
    property real radius: 0
    property real topLeftRadius: radius
    property real topRightRadius: radius
    property real bottomLeftRadius: radius
    property real bottomRightRadius: radius
    property bool ripple: true
    property bool dragged: false

    hoverEnabled: true
    cursorShape: enabled ? Qt.PointingHandCursor : Qt.ArrowCursor

    readonly property real _opacity: !enabled ? 0
        : dragged ? Theme.stateLayer.dragged
        : pressed ? Theme.stateLayer.pressed
        : containsMouse ? Theme.stateLayer.hover
        : activeFocus ? Theme.stateLayer.focus : 0

    Rectangle {
        anchors.fill: parent
        color: root.color
        opacity: root._opacity
        topLeftRadius: root.topLeftRadius
        topRightRadius: root.topRightRadius
        bottomLeftRadius: root.bottomLeftRadius
        bottomRightRadius: root.bottomRightRadius
        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
    }

    // The ripple is created on first press: there are hundreds of StateLayers in lists (a track row has several),
    // but only a few are pressed — the wave, mask and animations of each would cost on every page open.
    // The layer is enabled only for the duration of the animation, to avoid holding extra FBOs.
    property Item _ripple: null
    // the wave's place is under the content nested in the StateLayer
    Item { id: rippleSlot; anchors.fill: parent }

    Component {
        id: rippleComponent
        Item {
            id: rippleHost
            anchors.fill: parent
            visible: wave.opacity > 0
            layer.enabled: visible
            layer.effect: MultiEffect {
                maskEnabled: true
                maskSource: mask
                maskThresholdMin: 0.5
                maskSpreadAtMin: 1.0
            }

            function start(x, y) {
                fade.stop()
                wave.cx = x
                wave.cy = y
                grow.restart()
            }
            function release() { fade.start() }

            Rectangle {
                id: wave
                property real cx: 0
                property real cy: 0
                readonly property real maxR: Math.hypot(Math.max(cx, root.width - cx), Math.max(cy, root.height - cy))
                x: cx - width / 2
                y: cy - height / 2
                width: 2 * maxR * scaleFactor
                height: width
                radius: width / 2
                color: root.color
                opacity: 0
                property real scaleFactor: 0
            }

            Rectangle {
                id: mask
                parent: rippleSlot      // next to the wave, not inside its layer
                anchors.fill: parent
                visible: false
                // mask layer — only while the ripple is visible
                layer.enabled: rippleHost.visible
                topLeftRadius: root.topLeftRadius
                topRightRadius: root.topRightRadius
                bottomLeftRadius: root.bottomLeftRadius
                bottomRightRadius: root.bottomRightRadius
            }

            ParallelAnimation {
                id: grow
                NumberAnimation {
                    target: wave; property: "scaleFactor"; from: 0.1; to: 1
                    duration: 450
                    easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedDecelerate
                }
                NumberAnimation { target: wave; property: "opacity"; to: Theme.stateLayer.pressed; duration: 75 }
            }

            NumberAnimation {
                id: fade
                target: wave; property: "opacity"; to: 0; duration: 300
            }
        }
    }

    onPressed: mouse => {
        if (!ripple)
            return
        if (!_ripple)
            _ripple = rippleComponent.createObject(rippleSlot)
        _ripple.start(mouse.x, mouse.y)
    }
    onReleased: if (_ripple) _ripple.release()
    onCanceled: if (_ripple) _ripple.release()
}
