import QtQuick
import Md3

// Source for dragging a track into the queue: the "chip" with the cover follows the cursor above the window.
// The targets are QueueView and the mini player's queue button: DropArea with the "materialfy/track" key,
// the track is taken from drop.source.trackData() (source is the row with trackData()).
Item {
    id: root

    property Item source
    property string title
    property string cover
    readonly property alias active: drag.active

    DragHandler {
        id: drag
        target: null
        acceptedDevices: PointerDevice.Mouse
        acceptedButtons: Qt.LeftButton
        onActiveChanged: {
            if (active) {
                ghostLoader.active = true
                const ghost = ghostLoader.item
                ghost.parent = root.Window.contentItem ?? root
                ghost.place(centroid.scenePosition)
                ghost.Drag.active = true
            } else if (ghostLoader.item) {
                ghostLoader.item.Drag.drop()
                ghostLoader.active = false
            }
        }
        onCentroidChanged: if (active && ghostLoader.item) ghostLoader.item.place(centroid.scenePosition)
    }

    // The chip is created only for the duration of the drag: there are many rows in the list, and only one is dragged
    // (previously each row kept its own hidden chip with a shadow and a second copy of the cover)
    Loader {
        id: ghostLoader
        active: false
        sourceComponent: ghostComponent
    }

    Component {
        id: ghostComponent
        Item {
            id: ghost
            z: 10000
            Drag.keys: ["materialfy/track"]
            Drag.source: root.source
            function place(p) {
                const q = parent.mapFromItem(null, p.x, p.y)
                x = q.x
                y = q.y
            }
            Elevation { target: chip; level: 3 }
            Rectangle {
                id: chip
                x: 14
                y: 10
                width: chipRow.implicitWidth + 20
                height: 48
                radius: 24
                color: Theme.surfaceContainerHighest
                Row {
                    id: chipRow
                    x: 8
                    anchors.verticalCenter: parent.verticalCenter
                    spacing: 10
                    MorphImage {
                        width: 32
                        height: 32
                        source: root.cover
                        shape: "circle"
                    }
                    Label {
                        anchors.verticalCenter: parent.verticalCenter
                        width: Math.min(implicitWidth, 240)
                        elide: Text.ElideRight
                        text: root.title
                        type: "labelLarge"
                        color: Theme.fgSurface
                    }
                }
            }
        }
    }
}
