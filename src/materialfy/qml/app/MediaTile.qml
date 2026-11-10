import QtQuick
import QtQuick.Layouts
import Md3

// Album / artist / playlist tile: cover in an MD3E shape, title, sublabel.
// On hover the shape morphs, for artists it is a circle. showPlay — the "Play" button on the cover on hover;
// badge — a corner plate ("RADIO"), pinned — a pin (local pinning). Right click — menuRequested(window point).
Item {
    id: root

    property string cover
    property string title
    property string subtitle
    property bool round: false          // artist
    property bool explicit: false
    property string placeholderIcon: "album"
    // what is on the cover — for the fallback cover not from Spotify (MorphImage.fallbackKind/Artist/Title)
    property string coverKind
    property string coverArtist
    property string coverTitle
    property real size: 168
    property var shape: round ? "circle" : "softSquare"
    property var hoverShape: "softSquare"
    property bool showPlay: false
    property bool playing: false        // playing from this tile — a pause icon
    property string badge
    property bool pinned: false
    readonly property bool hovered: area.containsMouse || (playLoader.item !== null && playLoader.item.hovering)

    signal clicked()
    signal playClicked()
    signal menuRequested(point point)

    implicitWidth: size
    implicitHeight: col.implicitHeight

    ColumnLayout {
        id: col
        width: root.size
        spacing: 8

        Item {
            Layout.preferredWidth: root.size
            Layout.preferredHeight: root.size

            MorphImage {
                anchors.fill: parent
                visible: root.cover !== ""
                source: root.cover
                fallbackKind: root.coverKind
                fallbackArtist: root.coverArtist
                fallbackTitle: root.coverTitle
                shape: root.hovered ? root.hoverShape : root.shape
                duration: 0
            }
            // Placeholder, plate, pin and "Play" are created only when needed: there are hundreds of tiles on the shelves,
            // while coverless, plated or hovered ones are a handful
            Loader {
                anchors.fill: parent
                active: root.cover === ""
                sourceComponent: MorphShape {
                    shape: root.hovered ? root.hoverShape : root.shape
                    color: Theme.surfaceContainerHigh
                    Icon { anchors.centerIn: parent; name: root.placeholderIcon; size: root.size * 0.3; color: Theme.fgSurfaceVariant }
                }
            }

            Loader {
                x: 8
                y: 8
                active: root.badge !== ""
                sourceComponent: Rectangle {
                    implicitWidth: badgeLabel.implicitWidth + 12
                    implicitHeight: 20
                    radius: 6
                    color: Theme.primary
                    Label { id: badgeLabel; anchors.centerIn: parent; text: root.badge; type: "labelSmall"; weight: 700; color: Theme.fgPrimary }
                }
            }
            Loader {
                anchors.right: parent.right
                anchors.top: parent.top
                anchors.margins: 8
                active: root.pinned
                sourceComponent: Rectangle {
                    width: 28
                    height: 28
                    radius: 14
                    color: Theme.primaryContainer
                    Icon { anchors.centerIn: parent; name: "push_pin"; size: 16; fill: 1; color: Theme.fgPrimaryContainer }
                }
            }

            // "Play" on hover: it pops up from the bottom and grows (OutBack)
            Loader {
                id: playLoader
                anchors.right: parent.right
                anchors.bottom: parent.bottom
                anchors.margins: 8
                width: 48
                height: 48
                active: root.showPlay && (root.hovered || root.playing || item !== null)
                sourceComponent: Item {
                    id: playHost
                    readonly property bool hovering: playArea.containsMouse
                    // a hover-created button pops up animated, an already playing one — in place at once
                    property bool _armed: root.playing
                    Component.onCompleted: Qt.callLater(() => _armed = true)
                    readonly property bool shown: _armed && (root.hovered || root.playing)
                    visible: opacity > 0
                    opacity: shown ? 1 : 0
                    scale: shown ? 1 : 0.8
                    transform: Translate { y: playHost.shown ? 0 : 8; Behavior on y { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutBack } } }
                    Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }
                    Behavior on scale { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutBack } }
                    PlayButton {
                        anchors.fill: parent
                        size: 48
                        playing: root.playing
                        spin: false
                        pausedShape: "softSquare"
                        playingShape: "softSquare"
                        onClicked: root.playClicked()
                    }
                    MouseArea { id: playArea; anchors.fill: parent; hoverEnabled: true; acceptedButtons: Qt.NoButton }
                }
            }
        }

        RowLayout {
            Layout.preferredWidth: root.size
            spacing: 4
            Label {
                id: titleLabel
                // no wider than the tile (minus the "E" plate): a long title does not spill onto the neighbor, it fades
                Layout.preferredWidth: Math.min(Math.ceil(naturalWidth), root.size - (root.explicit ? 20 : 0))
                Layout.maximumWidth: Layout.preferredWidth
                Layout.alignment: root.round ? Qt.AlignHCenter : Qt.AlignLeft
                text: root.title
                type: "titleSmall"
                fade: true
                horizontalAlignment: root.round && !overflowing ? Text.AlignHCenter : Text.AlignLeft
            }
            Rectangle {
                visible: root.explicit
                implicitWidth: 16
                implicitHeight: 16
                radius: 4
                color: Theme.surfaceContainerHighest
                Label { anchors.centerIn: parent; text: "E"; type: "labelSmall"; color: Theme.fgSurfaceVariant }
            }
            Item { Layout.fillWidth: true; visible: !root.round }
        }
        Label {
            visible: root.subtitle !== ""
            Layout.preferredWidth: root.size
            Layout.topMargin: -6
            text: root.subtitle
            type: "bodySmall"
            color: Theme.fgSurfaceVariant
            fade: true
            horizontalAlignment: root.round && !overflowing ? Text.AlignHCenter : Text.AlignLeft
        }
    }

    MouseArea {
        id: area
        z: -1
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        onClicked: mouse => {
            if (mouse.button === Qt.RightButton)
                root.menuRequested(root.mapToItem(null, mouse.x, mouse.y))
            else
                root.clicked()
        }
    }
}
