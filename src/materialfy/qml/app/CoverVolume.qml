import QtQuick
import Md3
import Materialfy.Core

// Volume right on the cover (island, taskbar widget): the cover darkens, from the bottom up to the volume level
// an accent-colored "liquid" rises with a running wave on top; on a large cover there are percentages too.
// Shown for a second after each change (show()), then fades out. Placed over the cover
// (anchors.fill), radius — the rounding of its shape (a circle for the "cookie" is half the side).
Item {
    id: root

    property real level: Player.volume
    property color accent: Theme.primary
    property real radius: width / 2
    property bool motion: !Theme.reducedMotion && !Theme.lowPower
    // percentages from 44 px up: on a smaller cover they are unreadable, an icon goes there
    readonly property bool big: width >= 44

    function show() {
        shown = true
        hold.restart()
    }

    property bool shown: false
    property real _level: level
    Behavior on _level { enabled: root.motion; NumberAnimation { duration: 220; easing.type: Easing.OutCubic } }
    property real _phase: 0

    visible: opacity > 0.01
    opacity: shown ? 1 : 0
    Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }

    Timer { id: hold; interval: 1100; onTriggered: root.shown = false }

    // the wave runs only while the level is on screen
    Timer {
        interval: 33
        repeat: true
        running: root.visible && root.motion
        onTriggered: {
            root._phase += 0.32
            liquid.requestPaint()
        }
    }
    on_LevelChanged: if (visible) liquid.requestPaint()
    onVisibleChanged: if (visible) liquid.requestPaint()

    Canvas {
        id: liquid
        anchors.fill: parent
        renderStrategy: Canvas.Cooperative
        onPaint: {
            const ctx = getContext("2d")
            const w = width, h = height
            ctx.reset()
            if (w <= 0 || h <= 0)
                return
            const r = Math.min(root.radius, w / 2, h / 2)
            ctx.beginPath()
            ctx.roundedRect(0, 0, w, h, r, r)
            ctx.clip()
            ctx.fillStyle = Qt.rgba(0, 0, 0, 0.45)
            ctx.fillRect(0, 0, w, h)
            const v = Math.max(0, Math.min(1, root._level))
            if (v <= 0)
                return
            const amp = Math.max(1, h * 0.045)
            const top = h * (1 - v) + (v >= 1 ? -amp : 0)
            const len = Math.max(10, w / 2.2)
            ctx.beginPath()
            ctx.moveTo(0, h)
            for (let x = 0; x <= w + 1; x += 2)
                ctx.lineTo(x, top + amp * Math.sin(x / len * 2 * Math.PI + root._phase))
            ctx.lineTo(w, h)
            ctx.closePath()
            ctx.fillStyle = root.accent
            ctx.globalAlpha = 0.9
            ctx.fill()
        }
    }

    // percentages on a dark plate: readable even on a light "liquid"
    Rectangle {
        anchors.centerIn: parent
        visible: root.big
        width: pct.implicitWidth + 12
        height: pct.implicitHeight + 2
        radius: height / 2
        color: Qt.rgba(0, 0, 0, 0.42)
        Label {
            id: pct
            anchors.centerIn: parent
            text: Math.round(root.level * 100) + "%"
            type: "labelLarge"
            weight: 700
            font.features: { "tnum": 1 }
            color: "#ffffff"
        }
    }
    Icon {
        anchors.centerIn: parent
        visible: !root.big
        name: root.level <= 0 ? "volume_off" : root.level < 0.4 ? "volume_down" : "volume_up"
        size: Math.max(12, Math.round(root.width * 0.55))
        fill: 1
        color: "#ffffff"
    }
}
