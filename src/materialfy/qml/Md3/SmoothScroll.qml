import QtQuick

// Smooth scrolling for any Flickable/ListView: SmoothScroll { flickable: list }.
// Mouse wheel: a click moves the target by wheelStep, contentY catches up to it exponentially
// (regardless of frame rate — the same at 60 and 144 Hz); quick clicks accumulate.
// Touchpad: pixels 1:1 without delay. Plus a thin scroll indicator.
// Dragging the content with the mouse is best disabled on the Flickable itself: acceptedButtons: Qt.NoButton.
Item {
    id: root

    required property Flickable flickable
    property real wheelStep: 150          // px per wheel click
    property real smoothing: 14           // 1/s: higher — faster catch-up
    property real maxBoost: 2             // fast wheel spinning boosts the step up to wheelStep x maxBoost
    property real _recent: 0              // "clicks" in the last ~0.25 s (with decay)
    property real _lastWheel: 0
    property bool showScrollBar: true

    readonly property real minY: flickable.originY
    readonly property real maxY: flickable.originY + Math.max(0, flickable.contentHeight - flickable.height)
    property real _target: 0

    // --debug-wheel: print raw wheel events
    readonly property bool debugWheel: typeof materialfyDebugWheel !== "undefined" && materialfyDebugWheel

    signal userScrolled()   // wheel, touchpad or indicator — not a programmatic scroll

    function clampY(y) { return Math.max(minY, Math.min(maxY, y)) }

    // Touchpad: move immediately, without smoothing
    property real _fineUntil: 0           // until this moment the event stream has no phases — a Windows touchpad
    property bool _momentum: false        // in this gesture the system sends the momentum itself
    property var _samples: []             // [{ t, d }] — finger movements over the last ~0.1 s
    function _direct(dy) {
        smooth.running = false
        flickable.contentY = clampY(flickable.contentY + dy)
        _target = flickable.contentY
        directPulse.restart()
    }
    Timer { id: directPulse; interval: 500 }   // the scroll indicator is also visible during touchpad moves
    function _sample(now, d) {
        _samples.push({ t: now, d: d })
        while (_samples.length > 0 && now - _samples[0].t > 100)
            _samples.shift()
    }
    // px/s by the sign of pixelDelta (flick(0, v) with v > 0 goes to the start, like a finger moving down)
    function _speed(now) {
        const s = _samples.filter(p => now - p.t <= 100)
        if (s.length < 2 || now - s[s.length - 1].t > 50)
            return 0
        const span = s[s.length - 1].t - s[0].t
        let sum = 0
        for (let i = 1; i < s.length; ++i)
            sum += s[i].d
        return span > 0 ? sum * 1000 / span : 0
    }

    function scrollTo(y, animated) {
        _target = clampY(y)
        if (animated === false) {
            smooth.running = false
            flickable.contentY = _target
        } else {
            smooth.running = true
        }
    }

    WheelHandler {
        parent: root.flickable
        target: null
        acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
        orientation: Qt.Vertical
        onWheel: event => {
            root.userScrolled()
            if (root.debugWheel)
                console.log(`wheel: type=${event.device.type} name="${event.device.name}" phase=${event.phase}`
                            + ` angle=${event.angleDelta.y} pixel=${event.pixelDelta.y} inverted=${event.inverted}`)
            // A touchpad is recognized by the gesture phase (Begin/Update/End exist only for finger scrolling).
            // Neither the device type nor pixelDelta is reliable: on Wayland the mouse wheel also sends
            // pixelDelta (~15 px per click), and some mice arrive with the "TouchPad" type.
            const now = Date.now()
            const touchpad = event.phase !== Qt.NoScrollPhase
            if (touchpad) {
                if (root.flickable.flicking)
                    root.flickable.cancelFlick()
                if (event.phase === Qt.ScrollBegin) {
                    root._samples = []
                    root._momentum = false
                } else if (event.phase === Qt.ScrollMomentum) {
                    root._momentum = true   // the momentum comes from the system itself (macOS)
                }
                const dy = event.pixelDelta.y
                if (dy !== 0) {
                    root._sample(now, dy)
                    root._direct(-dy)
                }
                // The finger is released but the system gives no momentum (Wayland, X11): we finish the scroll ourselves — flick
                // at the speed of the last ~0.1 s, like a finger on a touch screen
                if (event.phase === Qt.ScrollEnd && !root._momentum && ["osx", "macos"].indexOf(Qt.platform.os) < 0) {
                    const v = root._speed(now)
                    root._samples = []
                    if (Math.abs(v) > 120)
                        root.flickable.flick(0, v)
                }
                return
            }
            if (root.flickable.flicking)
                root.flickable.cancelFlick()
            // A Windows touchpad arrives without phases, like a wheel (WM_MOUSEWHEEL), but in fractions of a click of arbitrary
            // size, dozens of events per second (the system sends the post-release momentum the same
            // way). For it, smoothing and boost mean lagging behind the finger and a "stiff" scroll, so
            // any fraction of a click moves the list at once, proportionally (hi-res wheels are better off this way too: their
            // fractions are the smoothness), and for another 0.4 s after it — the whole stream. Smoothing — whole clicks only.
            const a = Math.abs(event.angleDelta.y)
            if (a % 120 !== 0)
                root._fineUntil = now + 400
            if (now < root._fineUntil && event.angleDelta.y !== 0) {
                root._direct(-event.angleDelta.y / 120 * root.wheelStep)
                return
            }
            // Whole clicks: the target moves by wheelStep per click, contentY catches up smoothly.
            // No angleDelta — recompute from pixels: Wayland gives 15 px per click.
            const notches = event.angleDelta.y !== 0 ? event.angleDelta.y / 120 : event.pixelDelta.y / 15
            // The boost is by the number of clicks in the last ~0.25 s, not by the interval between events,
            // otherwise a hi-res wheel would boost within a single click.
            root._recent = root._recent * Math.exp(-(now - root._lastWheel) / 250) + Math.abs(notches)
            root._lastWheel = now
            const boost = Math.max(1, Math.min(root.maxBoost, 1 + (root._recent - 2) * 0.25))
            const base = smooth.running ? root._target : root.flickable.contentY
            root._target = root.clampY(base - notches * root.wheelStep * boost)
            smooth.running = true
        }
    }

    FrameAnimation {
        id: smooth
        running: false
        onTriggered: {
            const target = root.clampY(root._target)
            const d = target - root.flickable.contentY
            if (Math.abs(d) < 0.5) {
                root.flickable.contentY = target
                running = false
                return
            }
            // Whole pixels: icons (the native rasterizer) are snapped to the pixel grid, while text and
            // images are not; with a fractional contentY they "shimmer" relative to each other.
            const step = d * (1 - Math.exp(-frameTime * root.smoothing))
            root.flickable.contentY = Math.round(root.flickable.contentY + (Math.abs(step) < 1 ? Math.sign(d) : step))
        }
    }

    // Thin scroll indicator: appears on movement and hover, dragged with the mouse.
    Item {
        id: bar
        parent: root.flickable
        visible: root.showScrollBar && root.flickable.contentHeight > root.flickable.height
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        anchors.margins: 4
        width: 12

        readonly property bool active: root.flickable.movingVertically || smooth.running || directPulse.running
                                      || barArea.containsMouse || barArea.pressed
        opacity: active ? 1 : 0
        Behavior on opacity { NumberAnimation { duration: bar.active ? Theme.motion.effectsFast : 600 } }

        readonly property real ratio: root.flickable.height / Math.max(root.flickable.contentHeight, 1)
        readonly property real thumbH: Math.max(32, height * ratio)
        readonly property real pos: root.maxY > root.minY ? (root.flickable.contentY - root.minY) / (root.maxY - root.minY) : 0

        Rectangle {
            readonly property bool wide: barArea.containsMouse || barArea.pressed
            anchors.right: parent.right
            width: wide ? 8 : 4
            radius: width / 2
            height: bar.thumbH
            y: (bar.height - height) * bar.pos
            color: Theme.fgSurfaceVariant
            opacity: wide ? 0.7 : 0.45
            Behavior on width { NumberAnimation { duration: Theme.motion.effectsFast } }
        }

        MouseArea {
            id: barArea
            anchors.fill: parent
            hoverEnabled: true
            preventStealing: true
            property real grabOffset: 0
            function yFor(mouseY) {
                const p = (mouseY - grabOffset) / Math.max(1, bar.height - bar.thumbH)
                return root.minY + Math.max(0, Math.min(1, p)) * (root.maxY - root.minY)
            }
            onPressed: mouse => {
                root.userScrolled()
                const thumbY = (bar.height - bar.thumbH) * bar.pos
                grabOffset = mouse.y >= thumbY && mouse.y <= thumbY + bar.thumbH ? mouse.y - thumbY : bar.thumbH / 2
                root.scrollTo(yFor(mouse.y), grabOffset === bar.thumbH / 2)
            }
            onPositionChanged: mouse => { if (pressed) root.scrollTo(yFor(mouse.y), false) }
        }
    }
}
