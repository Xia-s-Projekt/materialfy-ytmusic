import QtQuick
import Materialfy.Core

// Material Symbols Rounded icon by ligature name: Icon { name: "play_arrow" }.
// Centered on the actual glyph outline, not its font cell: some glyphs
// (favorite, queue_music…) are drawn above the cell center and look offset on a square backing.
// The correction is measured by IconMetrics (Python) from rendered pixels — FontMetrics lies for ligatures.
Item {
    id: root

    property string name
    property real size: 24
    property real fill: 0          // 0..1, animated FILL axis
    property real weight: 400      // 100..700
    property real grade: 0         // -25..200
    property color color: Theme.fgSurfaceVariant
    property bool opticalCenter: true
    // true — width follows the visible glyph outline (for icons next to text: even spacing)
    property bool tight: false
    readonly property real inkWidth: _ink.width > 0 ? _ink.width : size
    readonly property real inkHeight: _ink.height > 0 ? _ink.height : size

    Behavior on fill { NumberAnimation { duration: Theme.motion.effectsDefault } }

    implicitWidth: tight ? inkWidth : size
    implicitHeight: size

    readonly property rect _ink: opticalCenter && name !== "" ? IconMetrics.ink(name, size) : Qt.rect(0, 0, 0, 0)

    Text {
        id: glyph
        text: IconMetrics.glyph(root.name)
        color: root.color
        textFormat: Text.PlainText
        renderType: Text.NativeRendering  // distance-field (QtRendering) pokes holes in the variable font fill
        font.family: Theme.iconFamily
        font.pixelSize: root.size
        font.variableAxes: ({ "FILL": root.fill, "wght": root.weight, "GRAD": root.grade, "opsz": Math.max(20, Math.min(48, root.size)) })

        // Whole pixels: the native rasterizer otherwise blurs the edges.
        x: Math.round((root.width - implicitWidth) / 2 - root._ink.x)
        y: Math.round((root.height - implicitHeight) / 2 - root._ink.y)
    }
}
