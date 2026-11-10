import QtQuick
import QtQuick.Shapes
import Md3

// Dashed zone "Drag a track here — it will play next". active — a track is being dragged over it.
Item {
    id: root

    property bool active: false
    property string text: I18n.t("Drop a track here to play it next")

    implicitHeight: 72
    implicitWidth: 280

    Rectangle {
        anchors.fill: parent
        radius: Theme.shape.largeIncreased
        color: root.active ? Qt.alpha(Theme.primary, 0.08) : "transparent"
        Behavior on color { enabled: !Theme.transitioning; ColorAnimation { duration: Theme.motion.effectsDefault } }
    }

    Shape {
        anchors.fill: parent
        preferredRendererType: Shape.CurveRenderer
        ShapePath {
            strokeColor: root.active ? Theme.primary : Theme.outlineVariant
            strokeWidth: 2
            strokeStyle: ShapePath.DashLine
            dashPattern: [3, 3]
            fillColor: "transparent"
            PathRectangle {
                x: 1
                y: 1
                width: root.width - 2
                height: root.height - 2
                radius: Theme.shape.largeIncreased
            }
        }
    }

    Row {
        anchors.centerIn: parent
        width: Math.min(implicitWidth, root.width - 32)
        spacing: 10
        Icon {
            id: icon
            anchors.verticalCenter: parent.verticalCenter
            name: "add_to_queue"
            size: 22
            color: root.active ? Theme.primary : Theme.fgSurfaceVariant
        }
        Label {
            anchors.verticalCenter: parent.verticalCenter
            width: Math.min(implicitWidth, root.width - 32 - icon.width - 10)
            text: root.text
            type: "bodyMedium"
            color: root.active ? Theme.primary : Theme.fgSurfaceVariant
            wrapMode: Text.WordWrap
            elide: Text.ElideNone
            maximumLineCount: 2
        }
    }
}
