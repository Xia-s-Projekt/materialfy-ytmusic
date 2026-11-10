import QtQuick
import Materialfy.Core

// An image (cover) clipped to an MD3 shape with morphing.
// A single ShaderEffect (shaders/shapeimage.frag): the texture is the Image itself, the mask is computed analytically,
// the placeholder backing is the same shader. Previously each cover had two offscreen layers
// (mask + MultiEffect) plus another placeholder shape; the home page has dozens of such covers.
Item {
    id: root

    property url source
    readonly property alias status: image.status
    property alias shape: m.shape
    property real angle: 0
    property alias duration: m.duration
    property alias livingSpeed: m.livingSpeed
    property real pulse: 0                  // shape "breathing"; the image inside does not move

    // Rotation of the frame shape, degrees: the image inside stays still. Uniform only, the outline is not recomputed.
    // Slow rotation — MaskSpin.
    property real maskRotation: 0

    function jumpTo(next) { m._ready = false; shape = next; m._set(next); m._ready = true }

    // The image loads when the element is first shown: hidden tiles (collapsed "All" rows), closed
    // panels do not hold decoded covers. Once shown it is not unloaded. eager — load immediately.
    property bool eager: false

    // If it fails — another attempt (by then the network manager has moved to another Spotify mirror), then
    // a fallback cover by name (Covers: iTunes, Deezer, Yandex). A retry is marked #rN: Qt remembers a failed
    // address load and would return the same error for the same address; the fragment is not sent to the server. fallbackKind: "album" | "track" |
    // "artist"; fallbackArtist — the first artist; fallbackTitle — the album or track (empty for an artist).
    property string fallbackKind
    property string fallbackArtist
    property string fallbackTitle
    property int _tries: 0
    property url _fallback
    property string _fallbackKey
    onSourceChanged: {
        _tries = 0
        _fallback = ""
        _fallbackKey = ""
        retry.stop()
    }
    function _failed() {
        if (_tries < 1) {
            retry.restart()
        } else if (fallbackKind !== "" && fallbackArtist !== "" && _fallbackKey === "" && _fallback.toString() === "") {
            _fallbackKey = Covers.find(fallbackKind, fallbackArtist, fallbackTitle)
        }
    }
    Timer {
        id: retry
        interval: 1500
        onTriggered: root._tries += 1
    }
    Connections {
        target: root._fallbackKey !== "" ? Covers : null
        function onFound(key, url) {
            if (key === root._fallbackKey && url !== "")
                root._fallback = url
        }
    }

    property bool _shownOnce: false
    onVisibleChanged: if (visible) _shownOnce = true
    Component.onCompleted: if (visible) _shownOnce = true
    property color placeholderColor: Theme.surfaceContainerHighest

    ShapeMorph {
        id: m
        active: root.visible && root.opacity > 0
    }

    Image {
        id: image
        visible: false
        asynchronous: true
        fillMode: Image.PreserveAspectCrop
        // Load once the size is known: otherwise the first load went with size 0 (128 px), and right after
        // it a second one for the real size. The size rounds up to a step: a quarter of the highest power
        // of two (at least 16 px): 48 -> 48, 168 -> 192, 440 -> 448 — window resize steps do not reload
        // the image every time, and the extra pixels are at most ~1.5x. Each pixel costs memory three times:
        // the QImage in the Qt Quick cache and the texture (the D3D11 driver keeps its own copy too).
        function _fit(px) {
            const step = Math.max(16, Math.pow(2, Math.floor(Math.log2(Math.max(1, px)))) / 4)
            return Math.max(16, Math.ceil(px / step) * step)
        }
        readonly property bool _sized: root.width > 0 && root.height > 0
        // Decode gate: the real source is set only when ImageGate has given a slot.
        // _hold — requested and waiting for the queue; a null source goes nowhere, the request is dropped.
        property bool _gateOn: false
        property bool _hold: false
        readonly property string _want: !_sized || !(root.eager || root._shownOnce) || root.source.toString() === "" ? ""
              : root._fallback.toString() !== "" ? root._fallback
              : root._tries > 0 ? root.source + "#r" + root._tries : root.source
        function qmlContinue() {
            _gateOn = true
            image.source = _want
            if (image.status !== Image.Loading)
                _releaseGate(false)
        }
        function _releaseGate(clearLoading) {
            // a slot holds both the waiting (_hold) and the loaded (_gateOn) — otherwise slots leak:
            // the queue stalls and no cover after the first twelve ever loads
            if (_hold) {
                _hold = false
                ImageGate.cancel(image)   // a waiting item has no slot — just drop the request
            }
            if (_gateOn) {
                _gateOn = false
                if (clearLoading && image.status === Image.Loading)
                    image.source = ""
                ImageGate.release(image)
            }
        }
        function _requestGate() {
            if (_want === "") {
                _releaseGate(true)
                image.source = ""
            } else if (!_hold && !_gateOn) {
                // the queue holds the Image itself (it has qmlContinue): the queue calls continue on
                // whoever was queued — a request from root would stay silent in release and the cover would never load
                if (ImageGate.acquire(image)) {
                    _gateOn = true
                    image.source = _want
                } else {
                    _hold = true
                    // reuseItems: while the row waits for the queue, retainWhileLoading shows the previous
                    // entry — someone else's cover must not hang there until its turn
                    image.source = ""
                }
            } else if (_gateOn) {
                image.source = _want
            }
            // setSource with the same URL does nothing and statusChanged never comes: the slot
            // given for an already loaded cover would leak. The slot is only needed while it loads.
            if (_gateOn && image.status !== Image.Loading)
                _releaseGate(false)
        }
        on_WantChanged: _requestGate()
        // A hidden queued Image is removed from the queue, and asks for a slot again when shown.
        // This listens to root.visible: the Image itself is always visible:false and its own signal here
        // does not report the MorphImage hiding/showing.
        Connections {
            target: root
            function onVisibleChanged() { root.visible ? image._requestGate() : image._releaseGate(true) }
        }
        Component.onDestruction: _releaseGate(true)
        // source is driven only by the gate transitions: imperative assignment does not fight the _want
        // binding and does not create a source -> status -> release -> source loop.
        sourceSize: Qt.size(_fit(root.width * Screen.devicePixelRatio), _fit(root.height * Screen.devicePixelRatio))
        retainWhileLoading: true
        smooth: true
        property bool shown: false          // at least one frame has happened (on reload the previous one is visible)
        onStatusChanged: {
            if (status === Image.Ready)
                shown = true
            else if (status === Image.Error || status === Image.Null)
                shown = false
            if (status === Image.Ready || status === Image.Error)
                _releaseGate(false)
            if (status === Image.Error)
                root._failed()
        }
    }

    ShaderEffect {
        id: fx
        readonly property real pad: Math.ceil(Math.min(root.width, root.height) * 0.06) + 1
        x: -pad
        y: -pad
        width: root.width + 2 * pad
        height: root.height + 2 * pad
        visible: root.width > 0 && root.height > 0 && (ready > 0 || root.placeholderColor.a > 0)

        readonly property real ready: image.shown && image.source.toString() !== "" ? 1 : 0
        property real _shown: ready
        // on software rendering the cover appears at once: the fade-in of dozens of covers loading
        // at different times would hold the window redrawing for a second or two after the page opens
        Behavior on _shown { enabled: !Theme.reducedMotion && !Theme.lowPower; NumberAnimation { duration: 150 } }

        property var source: image
        property vector4d a0: m.a[0]
        property vector4d a1: m.a[1]
        property vector4d a2: m.a[2]
        property vector4d a3: m.a[3]
        property vector4d b0: m.b[0]
        property vector4d b1: m.b[1]
        property vector4d b2: m.b[2]
        property vector4d b3: m.b[3]
        property vector4d c0: m.c[0]
        property vector4d c1: m.c[1]
        property vector4d c2: m.c[2]
        property vector4d c3: m.c[3]
        property vector4d morph: Qt.vector4d(m.s, m.t, (root.angle + root.maskRotation) * Math.PI / 180, root.pulse)
        property vector4d misc: Qt.vector4d(m.time, 0, _shown, 0)
        // box.zw — PreserveAspectCrop: which share of the texture is visible per axis
        readonly property real _iw: Math.max(1, image.implicitWidth)
        readonly property real _ih: Math.max(1, image.implicitHeight)
        readonly property real _k: (root.width / Math.max(1, root.height)) / (_iw / _ih)
        property vector4d box: Qt.vector4d(width / Math.max(1, root.width), height / Math.max(1, root.height),
                                           _k < 1 ? _k : 1, _k > 1 ? 1 / _k : 1)
        property color fill: root.placeholderColor
        property color stroke: "transparent"
        fragmentShader: "shaders/shapeimage.frag.qsb"
    }
}
