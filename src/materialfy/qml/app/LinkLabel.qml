import QtQuick
import Md3
import Materialfy.Core

// Link text (artist, album): underlined on hover. Only the text itself is clickable,
// not the empty space to the right — there the click goes to the row below it.
Label {
    id: root

    property bool linkEnabled: true
    // Artist card on hover (via Catalog — the main window shows it)
    property string previewArtistId
    signal clicked()

    font.underline: area.containsMouse && linkEnabled

    MouseArea {
        id: area
        width: Math.min(root.width, root.implicitWidth)
        height: root.height
        enabled: root.linkEnabled
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: {
            hoverDelay.stop()
            Catalog.cancelPreview()
            root.clicked()
        }
        onContainsMouseChanged: {
            if (root.previewArtistId === "")
                return
            if (containsMouse) {
                hoverDelay.restart()
            } else {
                hoverDelay.stop()
                Catalog.cancelPreview()
            }
        }
    }
    Timer {
        id: hoverDelay
        interval: 600
        onTriggered: {
            const p = area.mapToItem(null, 0, area.height)
            Catalog.previewArtist(root.previewArtistId, p.x, p.y, area.width)
        }
    }
}
