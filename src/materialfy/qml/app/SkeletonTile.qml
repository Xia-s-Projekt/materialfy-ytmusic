import QtQuick
import QtQuick.Layouts
import Md3

// Card placeholder (cover + two lines) while there is no data.
ColumnLayout {
    property var shape: "softSquare"
    property real size: 156

    spacing: 10
    MorphShape {
        Layout.preferredWidth: parent.size
        Layout.preferredHeight: parent.size
        shape: parent.shape
        color: Theme.surfaceContainerHigh
    }
    Rectangle { Layout.preferredWidth: parent.size * 0.8; Layout.preferredHeight: 12; radius: 6; color: Theme.surfaceContainerHigh }
    Rectangle { Layout.preferredWidth: parent.size * 0.5; Layout.preferredHeight: 10; radius: 5; color: Theme.surfaceContainerHigh }
}
