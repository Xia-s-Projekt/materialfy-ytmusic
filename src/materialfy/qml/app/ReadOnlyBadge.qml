import QtQuick
import Md3

// A "read only" pill: rootlist folders and other people's playlists (they can only be changed in Spotify).
Rectangle {
    id: root

    property string text: I18n.t("read-only")
    property string icon: "lock"

    implicitWidth: row.implicitWidth + 16
    implicitHeight: 24
    radius: height / 2
    color: Theme.surfaceContainerHigh

    Row {
        id: row
        anchors.centerIn: parent
        spacing: 4
        Icon { anchors.verticalCenter: parent.verticalCenter; name: root.icon; size: 14; color: Theme.fgSurfaceVariant }
        Label { anchors.verticalCenter: parent.verticalCenter; text: root.text; type: "labelSmall"; color: Theme.fgSurfaceVariant }
    }
}
