import QtQuick
import "Shapes.js" as Shapes

// Shape morph state for the shape/shapeimage shaders (MorphShape, MorphImage).
// Current shape = mix(mix(A, B, s), C, t): a single number t is animated, the GPU computes the outline.
// An interrupted morph "freezes" in the pair (A, B, s) — without recomputing points on the CPU.
QtObject {
    id: root

    property var shape: "circle"
    property int duration: Theme.motion.spatialDefault
    property real overshoot: 1.4
    property real livingSpeed: 1           // blob "breathing" speed, 0 — freeze
    property bool active: true             // not visible — the morph waits until it appears, "breathing" is paused

    property var a: Shapes.pack("circle")
    property var b: a
    property var c: a
    property real s: 0
    property real t: 0
    property real time: 0
    property bool _living: false
    property var _pending: undefined       // shape changed while the element was hidden

    onActiveChanged: {
        if (active && _pending !== undefined) {
            const next = _pending
            _pending = undefined
            morphTo(next)
        }
    }

    // On creation the shape binding fires after the default value — without _ready every
    // shape would morph from a circle every time a row appears in a list.
    property bool _ready: false
    Component.onCompleted: { _set(shape); _ready = true }
    onShapeChanged: if (_ready) morphTo(shape)

    function _set(next) {
        _pending = undefined
        _anim.stop()
        const p = Shapes.pack(next)
        a = p; b = p; c = p
        s = 0; t = 0
        _living = Shapes.isLiving(next)
    }

    // Change the shape at once, without a morph
    function jumpTo(next) {
        _ready = false
        shape = next
        _set(next)
        _ready = true
    }

    function morphTo(next) {
        if (duration <= 0 || Theme.reducedMotion) {
            _set(next)
            return
        }
        // a hidden shape is not animated: the morph plays when it is shown
        if (!active) {
            _anim.stop()
            if (t !== 0 || s !== 0) {
                a = c; b = c; s = 0; t = 0
            }
            _pending = next
            return
        }
        if (_anim.running) {
            // interrupted morph: the dominant one of the pair (A, B) remains, joined by the previous target with share t
            a = s < 0.5 ? a : b
            b = c
            s = t
        } else {
            a = c; b = c; s = 0
        }
        c = Shapes.pack(next)
        t = 0
        _living = _living || Shapes.isLiving(next)
        _anim.restart()
    }

    property NumberAnimation _anim: NumberAnimation {
        target: root
        property: "t"
        from: 0
        to: 1
        duration: root.duration
        easing.type: Easing.OutBack
        easing.overshoot: root.overshoot
        onFinished: {
            root.a = root.c; root.b = root.c; root.s = 0; root.t = 0
            root._living = Shapes.isLiving(root.shape)
        }
    }

    property TickAnimation _breath: TickAnimation {
        running: root.active && root._living && root.livingSpeed > 0 && !Theme.quiet && !Theme.reducedMotion
        onTriggered: root.time += frameTime * root.livingSpeed
    }
}
