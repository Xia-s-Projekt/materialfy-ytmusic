import QtQuick
import QtQuick.Layouts
import Md3

// Library row (the "list" view): a 48 cover (artist — circle, folder — icon, Liked — gradient),
// title + pin / "read only", subtitle, the "Owner", "Added", "Played" columns (the last one
// hides in a narrow window), ⋯ on hover. entry is a LIBRARY_KEYS dictionary.
Item {
    id: root

    property var entry: ({})
    property bool showPlayed: width >= 860
    property bool showAdded: width >= 640
    readonly property string kind: entry.kind ?? ""

    signal clicked()
    signal menuRequested(point point)

    implicitHeight: 64

    function date(iso) {
        if (!iso)
            return "—"
        const d = new Date(iso)
        if (isNaN(d.getTime()))
            return "—"
        return I18n.date(d)
    }
    function ago(iso) {
        if (!iso)
            return "—"
        const d = new Date(iso)
        if (isNaN(d.getTime()))
            return "—"
        return I18n.relative(d)
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 12
        anchors.rightMargin: 12
        spacing: 16

        RowLayout {
            Layout.fillWidth: true
            Layout.preferredWidth: 3
            spacing: 12

            Item {
                Layout.preferredWidth: 48
                Layout.preferredHeight: 48
                Rectangle {
                    anchors.fill: parent
                    visible: root.kind === "liked"
                    radius: 12
                    gradient: Gradient {
                        orientation: Gradient.Horizontal
                        GradientStop { position: 0; color: Theme.primaryContainer }
                        GradientStop { position: 1; color: Theme.tertiaryContainer }
                    }
                    Icon { anchors.centerIn: parent; name: "favorite"; size: 22; fill: 1; color: Theme.fgPrimaryContainer }
                }
                MorphImage {
                    anchors.fill: parent
                    visible: root.kind !== "liked" && (root.entry.cover ?? "") !== ""
                    source: visible ? root.entry.cover : ""
                    shape: root.kind === "artist" ? "circle" : "softSquare"
                }
                MorphShape {
                    anchors.fill: parent
                    visible: root.kind !== "liked" && (root.entry.cover ?? "") === ""
                    shape: root.kind === "artist" ? "circle" : "softSquare"
                    color: Theme.surfaceContainerHigh
                    Icon {
                        anchors.centerIn: parent
                        name: ({ playlist: "queue_music", album: "album", artist: "person", show: "podcasts" })[root.kind] ?? "music_note"
                        size: 22
                        color: Theme.fgSurfaceVariant
                    }
                }
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 2
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 6
                    Label {
                        Layout.fillWidth: implicitWidth > width
                        Layout.maximumWidth: Math.ceil(implicitWidth)
                        text: root.kind === "liked" ? I18n.t("Liked Songs") : (root.entry.title ?? "")
                        type: "bodyLarge"
                        weight: 500
                    }
                    Icon { visible: root.entry.pinned === true; name: "push_pin"; size: 18; fill: 1; color: Theme.primary }
                    Item { Layout.fillWidth: true }
                }
                Label {
                    Layout.fillWidth: true
                    text: root.entry.subtitle ?? ""
                    type: "bodyMedium"
                    color: Theme.fgSurfaceVariant
                }
            }
        }

        Label {
            Layout.fillWidth: true
            Layout.preferredWidth: 1.4
            text: root.kind === "liked" ? I18n.t("You") : (root.entry.owner ?? "")
            type: "bodyMedium"
            color: Theme.fgSurfaceVariant
        }
        Label {
            visible: root.showAdded
            Layout.preferredWidth: 120
            text: root.date(root.entry.addedAt)
            type: "bodyMedium"
            color: Theme.fgSurfaceVariant
        }
        Label {
            visible: root.showPlayed
            Layout.preferredWidth: 120
            text: root.ago(root.entry.playedAt)
            type: "bodyMedium"
            color: Theme.fgSurfaceVariant
            // "Last week" wraps to two lines instead of an ellipsis
            wrapMode: Text.WordWrap
            maximumLineCount: 2
        }
        IconButton {
            id: more
            Layout.preferredWidth: 32
            Layout.preferredHeight: 32
            icon: "more_horiz"
            size: "xs"
            iconSize: 26
            iconWeight: 700
            opacity: hover.hovered || pressed ? 1 : 0
            onClicked: root.menuRequested(more.mapToItem(null, 0, more.height))
        }
    }

    HoverHandler { id: hover }
    TapHandler {
        acceptedButtons: Qt.RightButton
        onTapped: point => root.menuRequested(root.mapToItem(null, point.position.x, point.position.y))
    }
    StateLayer {
        z: -1
        anchors.fill: parent
        radius: Theme.shape.large
        onClicked: root.clicked()
    }
}
