import QtQuick

// Theme change via a snapshot: a snapshot of source with the old colors is placed on top, the theme switches
// instantly beneath it, and the snapshot leaves. Cheaper than animating colors smoothly: all bindings are recalculated
// once instead of every frame, then it is a single textured layer. How the snapshot leaves — mode:
//   wipe     — a curtain toward direction;
//   ripple   — the new one spreads as a shape from the center of originItem (without it — the window center);
//   dissolve — the snapshot fades evenly;
//   liquid   — the snapshot fades along smooth noise and "drifts", the wave moving toward direction.
// Wiring: Theme.wipe = themeWipe (source is the whole window content, the ThemeWipe itself sits outside it, on top).
Item {
    id: root

    property Item source
    property string mode: "wipe"
    property Item originItem
    readonly property int duration: ({ ripple: 900, dissolve: 1000, liquid: 1200 })[mode] ?? 650
    readonly property bool running: _phase !== 0

    readonly property int _mode: Math.max(0, ["wipe", "ripple", "dissolve", "liquid"].indexOf(mode))
    property int _phase: 0          // 0 — none, 1 — grabbing the frame, 2 — the snapshot is leaving
    property int _frames: 0
    property real _dir: 1
    property point _origin: Qt.point(0.5, 0.5)
    property real _seed: 0
    property var _apply: null

    // The window is drawn: a curtain over a window that is not visible has no snapshot to take
    readonly property bool onScreen: visible && Window.window !== null && Window.window.visible
        && Window.visibility !== Window.Minimized

    // Start: ready(wipe) — the snapshot with the old colors is ready and lies on top; the theme switches beneath it,
    // and reveal() takes the snapshot away. Several windows change theme at once this way (Theme.floatingWipes).
    function start(direction, ready) {
        anim.stop()
        effect.progress = 0
        _dir = direction < 0 ? -1 : 1
        _apply = ready
        _frames = 0
        _seed = Math.random() * 50
        const it = originItem
        if (it && it.visible && width > 0 && height > 0) {
            const p = mapFromItem(it, it.width / 2, it.height / 2)
            _origin = Qt.point(p.x / width, p.y / height)
        } else {
            _origin = Qt.point(0.5, 0.5)
        }
        _phase = 1
        snap.scheduleUpdate()
    }

    ShaderEffectSource {
        id: snap
        sourceItem: root._phase !== 0 ? root.source : null
        live: false
        hideSource: false
        visible: false
    }

    // Visible even while the snapshot is taken (with shown: 0): the source texture updates only when someone draws it.
    function reveal() {
        if (_phase === 2 && !anim.running)
            anim.restart()
    }

    ShaderEffect {
        id: effect
        anchors.fill: parent
        visible: root._phase !== 0
        property variant source: snap
        property real progress: 0
        property real dir: root._dir
        property real edge: 0.22
        property real skew: 0.12
        property real shown: root._phase === 2 ? 1 : 0
        property real mode: root._mode
        property real aspect: width / Math.max(1, height)
        property real seed: root._seed
        property point origin: root._origin
        fragmentShader: Qt.resolvedUrl("shaders/wipe.frag.qsb")

        NumberAnimation on progress {
            id: anim
            running: false
            from: 0
            to: 1
            duration: root.duration
            // the curtain and the wave start fast and take long to slow; the dissolve is even
            easing.type: root._mode >= 2 ? Easing.InOutSine : Easing.BezierSpline
            easing.bezierCurve: [0.2, 0.0, 0.0, 1.0, 1.0, 1.0]
            onFinished: root._phase = 0
        }
    }

    // The snapshot is rendered in sync with the nearest frame; on the second tick it is definitely ready.
    FrameAnimation {
        running: root._phase === 1
        onTriggered: {
            if (++root._frames < 2)
                return
            root._phase = 2
            const ready = root._apply
            root._apply = null
            if (ready)
                ready(root)
            else
                root.reveal()
        }
    }
}
