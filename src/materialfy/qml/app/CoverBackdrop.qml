import QtQuick
import Md3

// Blurred cover filling the whole item: the fullscreen player's background and the window's atmospheric underlay.
// The blur is an upscale of a tiny copy of the cover (_side texels per side) by the backdrop shader:
// no convolution, no fullscreen texture; the blur strength blur (0..1) sets the copy size.
// mode is the character of the blur:
//   gauss    — even, like a gaussian: smooth transitions without grid or banding;
//   glass    — frosted glass: the cover is guessable, with fine grain on top;
//   palette  — without the cover: a smooth gradient from the theme colors (they are taken from the cover anyway);
//   blobs    — large color spots (linear interpolation).
// flow (0..1) is "drifting": the image stays in place, only the blur shifts.
Item {
    id: root

    property string source
    property string mode: "gauss"
    property real blur: 1
    property real flow: 0.5
    property bool running: visible

    readonly property bool _palette: mode === "palette"
    readonly property bool ready: _palette || img.status === Image.Ready
    readonly property real _blur: _palette ? 1 : Math.max(0, Math.min(1, blur))
    readonly property bool _flowing: flow > 0 && _blur > 0.2   // a sharp cover has nothing to "drift"
    property real _t: 0

    readonly property int _side: {
        const b = _blur
        if (mode === "blobs")
            return Math.round(12 + 788 * Math.pow(1 - b, 3))
        const min = mode === "glass" ? 14 : 6
        return Math.round(min * Math.pow(800 / min, Math.pow(1 - b, 1.5)))
    }

    Image {
        id: img
        width: root._side
        height: root._side
        // for weak blur a larger cover (coverLarge) is better — the owner passes it
        source: root._palette ? "" : root.source
        sourceSize: Qt.size(root._side, root._side)
        asynchronous: true
        smooth: true
        visible: false
        layer.enabled: true
        layer.smooth: true
        layer.textureSize: Qt.size(root._side, root._side)
    }

    // Palette: a 4x3 grid of theme colors — the shader stretches it the same way as the copy of the cover
    Grid {
        id: swatches
        columns: 4
        width: 4
        height: 3
        visible: false
        layer.enabled: root._palette
        layer.smooth: true
        layer.textureSize: Qt.size(4, 3)
        Repeater {
            model: root._palette ? [
                "primaryPaletteKeyColor", "secondaryContainer", "tertiaryContainer", "tertiaryPaletteKeyColor",
                "primaryContainer", "primaryPaletteKeyColor", "secondaryPaletteKeyColor", "tertiaryContainer",
                "tertiaryPaletteKeyColor", "primaryContainer", "primaryPaletteKeyColor", "secondaryContainer"
            ] : []
            Rectangle {
                required property string modelData
                width: 1
                height: 1
                color: Theme[modelData]
            }
        }
    }

    // Static background without "drifting": the same tiny copy (img._side px), stretched bilinearly —
    // almost the same soft gradient as in the shader, but without a fullscreen pass in every frame.
    // Offscreen decode (sourceSize + asynchronous): a full-size cover in the GUI thread is a freeze.
    Image {
        id: staticImg
        anchors.fill: parent
        source: img.source
        sourceSize: Qt.size(root._side, root._side)
        asynchronous: true
        visible: !root._palette && staticImg.status === Image.Ready && root.ready && !root._flowing
        fillMode: Image.PreserveAspectCrop
        smooth: true
    }

    ShaderEffect {
        anchors.fill: parent
        // Without "drifting" the shader is not needed: every frame it samples and blends the whole window area.
        // A static copy of the cover (the same cubic blur interpolation) is cheaper by orders of magnitude.
        visible: root.ready && (root._palette || root._flowing)
        property var source: root._palette ? swatches : img
        property real time: root._t
        property real amp: root._flowing ? 0.08 * root._blur : 0
        property real inset: root._flowing ? 0.06 : 0
        property real cubic: root.mode === "blobs" ? 0 : 1
        property real saturation: root._palette ? 1 : 1.1 + 0.4 * (1 - root._blur)
        property real grain: root.mode === "glass" ? 0.05 : 0
        property size texSize: root._palette ? Qt.size(4, 3) : Qt.size(root._side, root._side)
        property point crop: root._palette ? Qt.point(1, 1)
                           : width >= height ? Qt.point(1, height / Math.max(1, width))
                                             : Qt.point(width / Math.max(1, height), 1)
        fragmentShader: Qt.resolvedUrl("shaders/backdrop.frag.qsb")
    }

    TickAnimation {
        running: root.running && root._flowing && root.ready && !Theme.quiet
        // update the shader only when _t has actually moved: the motion is barely noticeable,
        // while uniforms every frame repaint the whole window area; this way ~8 Hz instead of 30
        property real _acc: 0
        onTriggered: {
            _acc += frameTime * (0.15 + 0.85 * Math.min(1, root.flow))
            if (_acc >= 0.002) {
                root._t += _acc
                _acc = 0
            }
        }
    }
}
