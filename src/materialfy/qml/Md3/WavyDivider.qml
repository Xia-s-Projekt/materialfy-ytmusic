import QtQuick
import QtQuick.Shapes

// A wavy divider (like the section headings in the reference sidebar).
// The root is an Item, not a Shape: Shape takes its implicit size from the path → a binding loop in a Layout.
Item {
    id: root

    property color color: Theme.outlineVariant
    property real thickness: 1.5
    property real amplitude: 2
    property real wavelength: 14
    property real phase: 0

    implicitWidth: 120
    implicitHeight: 2 * amplitude + thickness + 2

    function _wave() {
        const pts = []
        const k = Math.PI * 2 / wavelength
        const cy = height / 2
        const x0 = thickness / 2, x1 = Math.max(x0, width - thickness / 2)
        for (let x = x0; x < x1; x += 1.5)
            pts.push(Qt.point(x, cy + amplitude * Math.sin(k * x + phase)))
        pts.push(Qt.point(x1, cy + amplitude * Math.sin(k * x1 + phase)))
        return pts
    }

    Shape {
        anchors.fill: parent
        preferredRendererType: Shape.CurveRenderer

        ShapePath {
            strokeColor: root.color
            strokeWidth: root.thickness
            fillColor: "transparent"
            capStyle: ShapePath.RoundCap
            joinStyle: ShapePath.RoundJoin
            PathPolyline { path: root._wave() }
        }
    }
}
