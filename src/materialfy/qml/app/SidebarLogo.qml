import QtQuick
import QtQuick.Shapes
import Md3
import "../Md3/Shapes.js" as Shapes

// Sidebar header: the "Wave-M" mark and the "Materialfy" wordmark (lockup as in wordmark.svg).
// The mark is a "collapse / expand the panel" button, as in ChatGPT and Claude: on hover or keyboard focus
// the cookie morphs into a circle, the "M" with the runner dissolves, and the action icon appears in its place.
// The morph and the icon swap are property transitions on Theme.motion tokens; with Theme.reducedMotion they are instant.
Item {
    id: root

    LogoPaths { id: logo }

    property real markSize: 40                 // cookie diameter
    property bool expanded: true               // true — the "collapse" action, false — "expand"
    property bool wordmarkVisible: expanded
    // Hover or focus; stands and snapshots may set it manually
    property bool highlighted: area.containsMouse || mark.activeFocus
    readonly property string actionIcon: expanded ? "left_panel_close" : "left_panel_open"
    readonly property string actionText: expanded ? I18n.t("Collapse sidebar") : I18n.t("Expand sidebar")
    readonly property alias markItem: mark

    signal activated()

    readonly property real _k: markSize / 480  // LogoPaths coordinates: the cookie is a 0..480 square
    readonly property bool _instant: Theme.reducedMotion

    implicitWidth: wordmark.x + wordmark.width
    implicitHeight: markSize

    Item {
        id: mark
        objectName: "sidebarLogoMark"
        width: root.markSize
        height: root.markSize
        anchors.verticalCenter: parent.verticalCenter

        activeFocusOnTab: true
        Accessible.role: Accessible.Button
        Accessible.name: root.actionText
        Accessible.description: "Materialfy"
        Accessible.onPressAction: root.activated()
        Keys.onPressed: e => {
            if (e.key === Qt.Key_Space || e.key === Qt.Key_Return || e.key === Qt.Key_Enter) {
                e.accepted = true
                root.activated()
            }
        }

        // 0 — mark (cookie + M), 1 — button (circle + action icon)
        property real t: root.highlighted ? 1 : 0
        Behavior on t {
            enabled: !root._instant
            NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized }
        }
        // The "M" fades out in the first half of the morph, the icon appears in the second (with a slight overlap,
        // so there is no empty frame) — without a smear of two glyphs
        readonly property real glyphOut: Math.max(0, 1 - t / 0.55)
        readonly property real iconIn: Math.max(0, (t - 0.45) / 0.55)

        // Press — a short squeeze with a spring
        scale: area.pressed ? 0.9 : 1
        Behavior on scale {
            enabled: !root._instant
            NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutBack; easing.overshoot: Theme.motion.overshoot }
        }

        // MD3 state layer (Lilac over Ultraviolet): hover 8 %, focus 10 %, pressed 10 %
        property real stateOpacity: area.pressed ? Theme.stateLayer.pressed
                                    : root.highlighted ? (mark.activeFocus && !area.containsMouse ? Theme.stateLayer.focus : Theme.stateLayer.hover) : 0
        Behavior on stateOpacity { enabled: !root._instant; NumberAnimation { duration: Theme.motion.effectsFast } }

        Shape {
            anchors.fill: parent
            preferredRendererType: Shape.CurveRenderer
            ShapePath {
                strokeColor: "transparent"
                strokeWidth: 0
                // gradient of icon.svg: (80, 40) → (432, 472) with the cookie at 16..496
                fillGradient: LinearGradient {
                    x1: mark.width * 64 / 480; y1: mark.height * 24 / 480
                    x2: mark.width * 416 / 480; y2: mark.height * 456 / 480
                    GradientStop { position: 0; color: Qt.tint(logo.ultraviolet[0], Qt.alpha(logo.lilac, mark.stateOpacity)) }
                    GradientStop { position: 1; color: Qt.tint(logo.ultraviolet[1], Qt.alpha(logo.lilac, mark.stateOpacity)) }
                }
                PathPolyline {
                    // the teeth turn by another half step as they smooth into a circle
                    path: Shapes.outline(logo.cookie, "circle", mark.t, 96, mark.width, mark.height, mark.t * Math.PI / 8, 0, 0, 0)
                }
            }
        }

        // Mark: the wave-M and the runner
        Shape {
            anchors.fill: parent
            preferredRendererType: Shape.CurveRenderer
            opacity: mark.glyphOut
            visible: opacity > 0
            scale: 0.6 + 0.4 * mark.glyphOut
            ShapePath {
                strokeColor: "transparent"
                strokeWidth: 0
                fillColor: logo.lilac
                scale: Qt.size(root._k, root._k)
                PathSvg { path: logo.glyph }
            }
            ShapePath {
                strokeColor: "transparent"
                strokeWidth: 0
                fillColor: logo.ember
                scale: Qt.size(root._k, root._k)
                PathSvg { path: logo.runner }
            }
        }

        Icon {
            anchors.centerIn: parent
            name: root.actionIcon
            size: 24
            weight: 500
            color: logo.lilac
            opacity: mark.iconIn
            visible: opacity > 0
            scale: 0.6 + 0.4 * mark.iconIn
            rotation: (1 - mark.iconIn) * (root.expanded ? 45 : -45)
        }

        // MD3 focus indicator: a 3 px ring with a 2 px gap, only on keyboard focus
        Rectangle {
            anchors.centerIn: parent
            width: parent.width + 10
            height: width
            radius: width / 2
            color: "transparent"
            border.width: 3
            border.color: Theme.secondary
            visible: mark.activeFocus
        }

        MouseArea {
            id: area
            anchors.fill: parent
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onPressed: tip.hide()
            onClicked: root.activated()
        }
    }

    // Wordmark in curves from wordmark.svg (without the mark), onSurface color; fades out in the rail
    Shape {
        id: wordmark
        x: mark.x + logo.textLeft * root._k
        width: logo.textWidth * root._k
        height: root.markSize
        anchors.verticalCenter: parent.verticalCenter
        preferredRendererType: Shape.CurveRenderer
        opacity: root.wordmarkVisible ? 1 : 0
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: root._instant ? 0 : Theme.motion.effectsFast } }
        ShapePath {
            strokeColor: "transparent"
            strokeWidth: 0
            fillColor: Theme.fgSurface
            scale: Qt.size(root._k, root._k)
            PathSvg { path: logo.text }
        }
        ShapePath {
            strokeColor: "transparent"
            strokeWidth: 0
            fillColor: logo.ember
            scale: Qt.size(root._k, root._k)
            PathSvg { path: logo.textDot }
        }
    }

    // MD3 tooltip (plain). It lives in the window root: the panel clips its own content, while in the rail
    // the tooltip stands to the right of the mark, past the panel edge. In the expanded panel — under the mark.
    Timer {
        id: tipDelay
        interval: 600
        onTriggered: tip.show()
    }
    Connections {
        target: area
        function onContainsMouseChanged() {
            if (area.containsMouse && !area.pressed)
                tipDelay.restart()
            else if (!mark.activeFocus) {
                tipDelay.stop()
                tip.hide()
            }
        }
    }
    Connections {
        target: mark
        function onActiveFocusChanged() {
            if (mark.activeFocus)
                tip.show()
            else if (!area.containsMouse)
                tip.hide()
        }
    }
    onActionTextChanged: if (tip.shown) tip.place()

    Rectangle {
        id: tip
        property bool shown: false
        function place() {
            const host = parent
            if (!host)
                return
            const p = root.expanded ? mark.mapToItem(host, 0, mark.height + 8)
                                    : mark.mapToItem(host, mark.width + 12, (mark.height - height) / 2)
            x = Math.max(8, Math.min(host.width - width - 8, Math.round(p.x)))
            y = Math.round(p.y)
        }
        function show() { if (root.visible) { place(); shown = true } }
        function hide() { tipDelay.stop(); shown = false }

        parent: root.Window.contentItem
        z: 1000
        width: tipLabel.implicitWidth + 16
        height: 24
        radius: Theme.shape.extraSmall
        color: Theme.inverseSurface
        opacity: shown && root.visible ? 1 : 0
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: root._instant ? 0 : Theme.motion.effectsFast } }

        Label {
            id: tipLabel
            anchors.centerIn: parent
            text: root.actionText
            type: "bodySmall"
            color: Theme.fgInverseSurface
        }
    }
}
