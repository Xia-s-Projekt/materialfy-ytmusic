import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Artist card on hovering their name: photo, followers, genre, "Open" and "Radio".
// Placed over the window; show(id, x, y, w) — under the link, hideSoon() — hide if the mouse is not on the card.
Item {
    id: root

    property QtObject artist: null
    readonly property var info: artist ? artist.info : ({})
    property bool shown: false

    function show(artistId, x, y, w) {
        hideTimer.stop()
        artist = Catalog.artist(artistId)
        card.x = Math.max(8, Math.min(x + w / 2 - card.width / 2, width - card.width - 8))
        card.y = y + card.height + 16 > height ? Math.max(8, y - card.height - 36) : y + 6
        shown = true
    }
    function hideSoon() { hideTimer.restart() }
    function hide() { shown = false }

    Timer { id: hideTimer; interval: 250; onTriggered: if (!cardHover.hovered) root.hide() }

    Rectangle {
        id: card
        width: 320
        height: content.implicitHeight + 32
        radius: Theme.shape.extraLarge
        color: Theme.surfaceContainerHigh
        visible: opacity > 0
        opacity: root.shown ? 1 : 0
        scale: root.shown ? 1 : 0.94
        transformOrigin: Item.Top
        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
        Behavior on scale { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutBack } }

        Elevation { target: card; level: 3 }
        HoverHandler {
            id: cardHover
            onHoveredChanged: if (!hovered) root.hideSoon()
        }

        ColumnLayout {
            id: content
            x: 16
            y: 16
            width: parent.width - 32
            spacing: 12

            RowLayout {
                spacing: 16
                MorphImage {
                    Layout.preferredWidth: 72
                    Layout.preferredHeight: 72
                    source: root.info.cover ?? ""
                    shape: cardHover.hovered ? "softSquare" : "circle"
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 2
                    Label {
                        Layout.fillWidth: true
                        text: root.info.name ?? ""
                        type: "titleLarge"
                        weight: 600
                    }
                    Label {
                        Layout.fillWidth: true
                        visible: text !== ""
                        text: {
                            const n = root.info.followers ?? 0
                            if (!(n > 0))
                                return ""
                            return I18n.nCompact(n, "{0} follower", "{0} followers")
                        }
                        type: "bodyMedium"
                        color: Theme.fgSurfaceVariant
                    }
                    Label {
                        Layout.fillWidth: true
                        visible: text !== ""
                        text: root.info.genres ?? ""
                        type: "bodySmall"
                        color: Theme.fgSurfaceVariant
                    }
                }
            }
            LoadingIndicator {
                Layout.alignment: Qt.AlignHCenter
                visible: root.artist !== null && root.artist.loading
                size: 36
            }
            RowLayout {
                spacing: 8
                Button {
                    text: I18n.t("Open")
                    icon: "person"
                    style: "tonal"
                    onClicked: {
                        root.hide()
                        Catalog.openArtist(root.info.artistId ?? "")
                    }
                }
                Button {
                    text: I18n.t("Radio")
                    icon: "radio"
                    enabled: !Radio.locked
                    onClicked: {
                        root.hide()
                        Radio.playStation("artist:" + (root.info.artistId ?? ""), root.info.name ?? "")
                    }
                }
            }
        }
    }
}
