import QtQuick
import Md3
import Materialfy.Core

// Codec of the playing track (OGG 320...) — a small plate above the time in the mini player.
// On a remote device there is no codec: RemoteLabel sits in that place instead.
Rectangle {
    visible: Player.hasTrack && Player.codec !== "" && !Player.remote
    implicitWidth: codec.implicitWidth + 10
    implicitHeight: 16
    radius: 5
    color: Theme.surfaceContainerHigh

    Label {
        id: codec
        anchors.centerIn: parent
        text: Player.codec
        type: "labelSmall"
        weight: 600
        color: Theme.fgSurfaceVariant
    }
}
