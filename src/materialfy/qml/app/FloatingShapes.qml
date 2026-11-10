import QtQuick
import Md3

// Header background: several large MD3 Expressive shapes slowly drift and rotate.
// The layout is deterministic from the seed (each playlist has its own). Movement — transforms only
// (x/y/rotation), the shape paths are not recomputed; the animation stops when running = false.
Item {
    id: root

    property color color: Theme.surfaceContainerHighest
    property int count: 6
    property string seed: ""
    property bool running: visible
    // The edges fade vertically (in their own coordinates, px): transparent above fadeFrom and below fadeTo,
    // fully visible after fadeIn / fadeOut from them. fadeIn = 0 — no fade.
    property real fadeFrom: 0
    property real fadeIn: 0
    property real fadeTo: height
    property real fadeOut: 0

    property real _t: 0

    // A simple deterministic PRNG (mulberry32) from a string
    function _rng(str) {
        let h = 1779033703 ^ str.length
        for (let i = 0; i < str.length; i++) {
            h = Math.imul(h ^ str.charCodeAt(i), 3432918353)
            h = (h << 13) | (h >>> 19)
        }
        let a = h >>> 0
        return () => {
            a = (a + 0x6D2B79F5) | 0
            let t = Math.imul(a ^ (a >>> 15), 1 | a)
            t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t
            return ((t ^ (t >>> 14)) >>> 0) / 4294967296
        }
    }

    readonly property var _specs: {
        const rnd = _rng(seed + "#")
        const shapes = ["cookie9", "flower8", "clover4", "cookie6", "sunny", "flower6", "cookie12", "scallop"]
        const out = []
        for (let i = 0; i < count; i++) {
            out.push({
                shape: shapes[Math.floor(rnd() * shapes.length)],
                // across the width — evenly over the bands, so the shapes do not clump together
                x: (i + 0.15 + 0.7 * rnd()) / count,
                y: 0.15 + 0.7 * rnd(),
                size: 0.55 + 0.6 * rnd(),          // share of the height
                ax: 18 + 30 * rnd(), ay: 10 + 22 * rnd(),
                sx: 0.05 + 0.08 * rnd(), sy: 0.04 + 0.07 * rnd(),
                ph: rnd() * 6.28,
                r0: rnd() * 360, rs: (rnd() < 0.5 ? -1 : 1) * (3 + 5 * rnd())
            })
        }
        return out
    }

    TickAnimation {
        running: root.running && !Theme.quiet
        onTriggered: root._t += frameTime
    }

    Repeater {
        model: root._specs
        MorphShape {
            required property var modelData
            readonly property real side: modelData.size * root.height
            width: side
            height: side
            x: modelData.x * root.width - side / 2 + modelData.ax * Math.sin(root._t * modelData.sx * 6.28 + modelData.ph)
            y: modelData.y * root.height - side / 2 + modelData.ay * Math.cos(root._t * modelData.sy * 6.28 + modelData.ph)
            // rotate the outline in the shader, not the item: the fade is computed along the shader vertical
            angle: modelData.r0 + root._t * modelData.rs
            shape: modelData.shape
            color: root.color
            // shader y 0..1 <-> [y - pad, y + side + pad] here
            readonly property real _top: y - pad
            readonly property real _span: side + 2 * pad
            fade: root.fadeIn > 0 ? Qt.vector4d((_top - root.fadeFrom) / root.fadeIn, _span / root.fadeIn,
                                                (root.fadeTo - _top) / root.fadeOut, -_span / root.fadeOut)
                                  : Qt.vector4d(0, 0, 0, 0)
        }
    }
}
