import QtQuick
import QtQuick.Layouts
import Md3

// A settings group: section heading plus a card.
ColumnLayout {
    id: root

    property string title
    property string icon
    default property alias rows: body.data

    Layout.fillWidth: true
    spacing: 12

    RowLayout {
        spacing: 8
        Icon { name: root.icon; size: 20; color: Theme.primary; visible: root.icon !== "" }
        Label { text: root.title; type: "titleMedium"; color: Theme.primary }
    }

    Rectangle {
        Layout.fillWidth: true
        implicitHeight: body.implicitHeight + 40
        radius: Theme.shape.extraLarge
        color: Theme.surfaceContainerHigh

        ColumnLayout {
            id: body
            x: 24
            y: 20
            width: parent.width - 48
            spacing: 20
        }
    }
}
