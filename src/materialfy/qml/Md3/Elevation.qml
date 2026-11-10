import QtQuick
import QtQuick.Effects

// MD3 level (0..5) shadow under the rectangular target surface.
RectangularShadow {
    id: root

    required property Item target
    property int level: 1
    readonly property real _dp: [0, 1, 3, 6, 8, 12][Math.max(0, Math.min(5, level))]

    parent: target.parent
    anchors.fill: target
    z: target.z - 1
    radius: target.radius ?? 0
    offset.y: _dp * 0.6
    blur: _dp * 2.2
    spread: 0
    color: Qt.alpha(Theme.shadow, 0.35)
    // The shadow is a separate element next to the surface, so it mirrors its visibility, opacity
    // and scale: otherwise a hidden card leaves a dark spot (one never shown leaves it at 0,0).
    visible: level > 0 && target.visible
    opacity: target.opacity
    scale: target.scale
    transformOrigin: target.transformOrigin
}
