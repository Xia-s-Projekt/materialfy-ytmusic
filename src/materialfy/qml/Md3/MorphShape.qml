import QtQuick

// MD3 Expressive shape. When shape changes it smoothly morphs from the current form to the new one
// (including from an intermediate one, if the previous morph has not finished yet).
// It draws the shaders/shape.frag shader: the outline is a polar function, morph, color, rotation, "breathing" —
// uniforms. Previously there was a PathPolyline of 120 points: JS rebuilt it every morph frame,
// CurveRenderer re-triangulated it, and a separate layer applied the color.
// The root is an Item: nested elements (icon, initials) sit on top of the shape.
Item {
    id: root

    property var shape: "circle"
    property real angle: 0                 // outline rotation, degrees (Item.rotation is left alone)
    property int samples: 120              // unused (the shader computes the outline), kept for compatibility
    property color color: Theme.primary
    property color strokeColor: "transparent"
    property bool tinted: true             // unused: the color is always a uniform
    property real strokeWidth: 0           // stroke inside the outline, px
    property alias duration: m.duration
    property alias overshoot: m.overshoot
    property alias livingSpeed: m.livingSpeed // blob shape "breathing" speed, 0 — freeze
    property real pulse: 0                 // 0..1 — outline "breathing": dents are deeper, the shape slightly wider
    // Vertical fade without a layer (see shape.frag): (u0, du, v0, dv) along the shader y, 0 — none
    property alias fade: fx.fade
    readonly property alias pad: fx.pad    // how far the shader extends past the item on each side, px

    function morphTo(next) { m.morphTo(next) }
    // Change the shape at once, without a morph (the m.shape binding is not broken)
    function jumpTo(next) { m._ready = false; shape = next; m._set(next); m._ready = true }

    implicitWidth: 48
    implicitHeight: 48

    ShapeMorph {
        id: m
        shape: root.shape
        active: root.visible && root.opacity > 0
    }

    ShaderEffect {
        id: fx
        // slightly larger than the item: the morph overshoot (OutBack) must not hit the rectangle edge
        readonly property real pad: Math.ceil(Math.min(root.width, root.height) * 0.06) + 1
        x: -pad
        y: -pad
        width: root.width + 2 * pad
        height: root.height + 2 * pad
        z: -1   // under the nested elements (icon, initials)
        visible: root.width > 0 && root.height > 0 && (root.color.a > 0 || root.strokeWidth > 0)
        blending: true

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
        property vector4d morph: Qt.vector4d(m.s, m.t, root.angle * Math.PI / 180, root.pulse)
        property vector4d misc: Qt.vector4d(m.time, root.strokeWidth, 1, 0)
        property vector4d box: Qt.vector4d(width / Math.max(1, root.width), height / Math.max(1, root.height), 1, 1)
        property color fill: root.color
        property color stroke: root.strokeColor
        property vector4d fade
        fragmentShader: "shaders/shape.frag.qsb"
    }
}
