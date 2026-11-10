import QtQuick
import QtQuick.Layouts
import Md3

// Queue section header: label, count badge, wavy line to the edge, optional button.
Item {
    id: root

    property string title
    property int count: -1                 // < 0 — no badge
    property string actionIcon
    property string actionText             // button tooltip
    property string note                   // line under the title ("New tracks go to the end")

    signal action()

    implicitHeight: col.implicitHeight + 16
    implicitWidth: 240

    ColumnLayout {
        id: col
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.leftMargin: 8
        anchors.rightMargin: 4
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 4
        spacing: 2

        RowLayout {
            Layout.fillWidth: true
            Layout.preferredHeight: 32
            spacing: 8

            Label {
                Layout.preferredWidth: implicitWidth
                Layout.maximumWidth: col.width - 96
                text: root.title
                type: "titleSmall"
                color: Theme.fgSurfaceVariant
            }
            Rectangle {
                visible: root.count >= 0
                implicitWidth: Math.max(20, countLabel.implicitWidth + 12)
                implicitHeight: 20
                radius: 10
                color: Theme.secondaryContainer
                Label {
                    id: countLabel
                    anchors.centerIn: parent
                    text: root.count
                    type: "labelSmall"
                    color: Theme.fgSecondaryContainer
                }
            }
            WavyDivider {
                Layout.fillWidth: true
                Layout.preferredHeight: 8
                Layout.minimumWidth: 16
            }
            IconButton {
                visible: root.actionIcon !== ""
                icon: root.actionIcon
                size: "xs"
                onClicked: root.action()
            }
        }
        Label {
            visible: root.note !== ""
            Layout.fillWidth: true
            text: root.note
            type: "bodySmall"
            color: Theme.fgSurfaceVariant
        }
    }
}
