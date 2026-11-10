import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// The large "Liked tracks" tile (two library grid cells): primaryContainer → tertiaryContainer gradient,
// track thumbnails, a halo shape with a heart (softSquare → softSquare on hover), "Play".
Item {
    id: root

    property int trackCount: Library.liked ? Library.liked.count : 0
    readonly property bool hovered: area.containsMouse || playArea.containsMouse

    signal clicked()
    signal playClicked()
    signal menuRequested(point point)

    implicitWidth: 358
    implicitHeight: 216

    // "Artist Track · …" from the first rows of Liked
    readonly property string sample: {
        const m = Library.liked
        const out = []
        for (let i = 0; m && i < Math.min(6, m.count); i++) {
            const t = m.get(i)
            out.push((t.artists ?? "") + " " + (t.title ?? ""))
        }
        return out.join(" · ")
    }

    Rectangle {
        id: card
        anchors.fill: parent
        radius: Theme.shape.extraLarge
        clip: true
        gradient: Gradient {
            orientation: Gradient.Horizontal
            GradientStop { position: 0; color: Theme.primaryContainer }
            GradientStop { position: 1; color: Theme.tertiaryContainer }
        }

        // Rectangle's clip is a rectangle, not the rounding: the shape must not reach the card corner,
        // otherwise its square edge shows past the rounded corner
        MorphShape {
            x: parent.width - width + 4
            y: -14
            width: 150
            height: 150
            shape: "softSquare"
            color: Qt.alpha(Theme.fgPrimaryContainer, 0.18)
            Icon {
                anchors.centerIn: parent
                anchors.horizontalCenterOffset: -2
                anchors.verticalCenterOffset: 7
                name: "favorite"
                size: 56
                fill: 1
                color: Theme.fgPrimaryContainer
            }
        }

        ColumnLayout {
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            anchors.leftMargin: 24
            anchors.rightMargin: 96
            anchors.bottomMargin: 20
            spacing: 2
            Label {
                visible: text !== ""
                Layout.fillWidth: true
                Layout.bottomMargin: 8
                text: root.sample
                type: "bodySmall"
                color: Theme.fgPrimaryContainer
                opacity: 0.85
                wrapMode: Text.WordWrap
                maximumLineCount: 2
                elide: Text.ElideRight
            }
            Label { text: I18n.t("Liked Songs"); type: "headlineMedium"; weight: 500; color: Theme.fgPrimaryContainer }
            Label {
                text: Library.liked && Library.liked.loading && root.trackCount === 0 ? I18n.t("Loading…")
                    : I18n.n(root.trackCount, "{0} track", "{0} tracks")
                type: "bodyMedium"
                color: Theme.fgPrimaryContainer
                opacity: 0.8
            }
        }
    }

    MouseArea {
        id: area
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        onClicked: mouse => mouse.button === Qt.RightButton ? root.menuRequested(root.mapToItem(null, mouse.x, mouse.y)) : root.clicked()
    }

    PlayButton {
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.margins: 20
        size: 56
        spin: false
        pausedShape: "softSquare"
        playingShape: "softSquare"
        playing: Player.playing && Player.source === "liked"
        onClicked: root.playClicked()
        MouseArea { id: playArea; anchors.fill: parent; hoverEnabled: true; acceptedButtons: Qt.NoButton }
    }
}
