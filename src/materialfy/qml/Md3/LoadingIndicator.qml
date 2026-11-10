import QtQuick

// MD3 Expressive loading indicator: the shape morphs through a cycle of presets and rotates.
// It only runs while visible (running && visible), otherwise it costs no frames.
Item {
    id: root

    property bool running: true
    property bool contained: false        // circular backing (the "contained" variant)
    property real size: 48
    property color color: contained ? Theme.fgPrimaryContainer : Theme.primary
    // reduced motion — a static shape
    readonly property bool active: running && visible && Theme.reducedMotion !== true

    readonly property var sequence: ["cookie9", "pentagonish", "pill", "sunny", "cookie4", "circle", "flower6"]
    property int _i: 0

    implicitWidth: size
    implicitHeight: size

    Rectangle {
        anchors.fill: parent
        radius: width / 2
        color: Theme.primaryContainer
        visible: root.contained
    }

    MorphShape {
        id: shape
        anchors.centerIn: parent
        width: root.size * (root.contained ? 0.6 : 0.79)
        height: width
        color: root.color
        samples: 72
        // software rendering: shapes swap without a morph, rotation runs off the Ticker clock (~30 Hz) rather than every
        // monitor frame: the CPU draws each frame there, and the indicator spins for the whole load
        duration: Theme.lowPower ? 0 : 500
        overshoot: 1.2
        shape: root._shapeAt(root._i)
        rotation: 0

        NumberAnimation on rotation {
            running: root.active && !Theme.lowPower
            from: 0
            to: 360
            duration: 2600
            loops: Animation.Infinite
        }
        TickAnimation {
            running: root.active && Theme.lowPower
            onTriggered: shape.rotation = (shape.rotation + frameTime * 360 / 2.6) % 360
        }
    }

    Timer {
        interval: 650
        repeat: true
        running: root.active
        onTriggered: root._i = (root._i + 1) % root.sequence.length
    }

    function _shapeAt(i) {
        const name = sequence[i]
        // shapes not in the presets are described inline
        if (name === "pentagonish")
            return { kind: "flower", n: 5, depth: 0.16, p: 3 }
        if (name === "pill")
            return { kind: "square", p: 2.4, aspect: 1.35 }
        return name
    }
}
