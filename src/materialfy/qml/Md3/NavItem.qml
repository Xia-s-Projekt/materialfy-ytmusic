import QtQuick

// Sidebar navigation item: a full-width pill; the selected one has the icon in a primary-colored square.
// The icon is always in the same place (iconInset from the left edge) — both expanded and in the rail,
// so when the sidebar collapses only the width changes and nothing jumps.
Item {
    id: root

    property string icon
    property string text
    property bool selected: false
    property bool compact: false          // the label and tail fade out, the pill shrinks with the width
    property real iconInset: 14           // from the left edge of the pill to the icon square (28 px)
    property real labelWidth: 180         // fixed: during the width animation the text does not rewrap or "jump" with an ellipsis
    property string trailingIcon
    property color trailingColor: Theme.primary
    property string imageSource

    signal clicked()
    // Right-click on the item: the row's action menu (pin, etc.); the point is in scene coordinates
    signal rightClicked(var point)

    implicitHeight: 48
    implicitWidth: 240
    clip: true

    Rectangle {
        anchors.fill: parent
        radius: height / 2
        color: root.selected ? Theme.secondaryContainer : "transparent"
        Behavior on color { enabled: !Theme.transitioning; ColorAnimation { duration: Theme.motion.effectsDefault } }
    }

    Item {
        id: iconBox
        x: root.iconInset
        width: 28
        height: 28
        anchors.verticalCenter: parent.verticalCenter

        Rectangle {
            anchors.fill: parent
            radius: 8
            color: Theme.primary
            scale: root.selected && root.imageSource === "" ? 1 : 0.4
            opacity: root.selected && root.imageSource === "" ? 1 : 0
            Behavior on scale { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutBack; easing.overshoot: 2 } }
            Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
        }
        Icon {
            anchors.centerIn: parent
            visible: root.imageSource === ""
            name: root.icon
            size: 20
            fill: root.selected ? 1 : 0
            color: root.selected ? Theme.fgPrimary : Theme.fgSurfaceVariant
        }
        // The cover (playlists) is only created when there is an image: a hidden MorphImage
        // still took part in every theme repaint frame.
        Loader {
            anchors.fill: parent
            active: root.imageSource !== ""
            sourceComponent: MorphImage {
                source: root.imageSource
                shape: "softSquare"
            }
        }
    }

    Label {
        x: iconBox.x + iconBox.width + 8   // icon 20 in a square of 28 -> visible gap ~12
        anchors.verticalCenter: parent.verticalCenter
        width: root.labelWidth - (root.trailingIcon !== "" ? 28 : 0)
        text: root.text
        type: "labelLarge"
        color: root.selected ? Theme.fgSecondaryContainer : Theme.fgSurfaceVariant
        opacity: root.compact ? 0 : 1
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
    }

    Icon {
        visible: root.trailingIcon !== "" && opacity > 0
        opacity: root.compact ? 0 : 1
        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
        anchors.right: parent.right
        anchors.rightMargin: 14
        anchors.verticalCenter: parent.verticalCenter
        name: root.trailingIcon
        size: 18
        fill: 1
        color: root.trailingColor
    }

    StateLayer {
        id: hit
        anchors.fill: parent
        radius: height / 2
        color: Theme.fgSurface
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        onClicked: mouse => {
            if (mouse.button === Qt.RightButton)
                root.rightClicked(root.mapToItem(null, mouse.x, mouse.y))
            else
                root.clicked()
        }
    }

    // In the rail there are no labels — the MD3 tooltip appears to the right of the item after a short delay. It lives in the window root:
    // the item and the panel clip their content.
    Timer {
        id: tipDelay
        interval: 500
        running: root.compact && hit.containsMouse && !hit.pressed && root.text !== ""
    }
    Rectangle {
        id: tip
        readonly property bool shown: root.compact && hit.containsMouse && !hit.pressed && !tipDelay.running && root.text !== ""
        parent: root.Window.contentItem
        z: 1000
        width: Math.min(tipLabel.implicitWidth + 16, 320)
        height: 24
        radius: Theme.shape.extraSmall
        color: Theme.inverseSurface
        opacity: shown ? 1 : 0
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
        onShownChanged: if (shown && parent) {
            const p = root.mapToItem(parent, root.width + 8, (root.height - height) / 2)
            x = Math.round(p.x)
            y = Math.round(p.y)
        }

        Label {
            id: tipLabel
            anchors.verticalCenter: parent.verticalCenter
            x: 8
            width: parent.width - 16
            text: root.text
            type: "bodySmall"
            color: Theme.fgInverseSurface
        }
    }
}
