import QtQuick
import Md3

// Non-interactive state chip: ok — primaryContainer + check, warn — errorContainer + block, neutral.
Rectangle {
    id: root

    property string text
    property string kind: "neutral"   // ok | warn | neutral
    property string icon: kind === "ok" ? "check" : kind === "warn" ? "block" : ""

    readonly property color _fg: kind === "ok" ? Theme.fgPrimaryContainer
                               : kind === "warn" ? Theme.fgErrorContainer : Theme.fgSurfaceVariant

    implicitWidth: row.implicitWidth + (icon !== "" ? 10 + 12 : 24)
    implicitHeight: 28
    radius: Theme.shape.small
    color: kind === "ok" ? Theme.primaryContainer : kind === "warn" ? Theme.errorContainer : Theme.surfaceContainerHighest
    Behavior on color { enabled: !Theme.transitioning; ColorAnimation { duration: Theme.motion.effectsDefault } }

    Row {
        id: row
        anchors.verticalCenter: parent.verticalCenter
        x: root.icon !== "" ? 10 : 12
        spacing: 6
        Icon {
            visible: root.icon !== ""
            anchors.verticalCenter: parent.verticalCenter
            name: root.icon
            size: 16
            color: root._fg
        }
        Label {
            anchors.verticalCenter: parent.verticalCenter
            text: root.text
            type: "labelLarge"
            color: root._fg
        }
    }
}
