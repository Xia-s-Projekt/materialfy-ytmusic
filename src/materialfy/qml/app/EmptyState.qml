import QtQuick
import QtQuick.Layouts
import Md3

// Empty state: an icon in morphing MD3E shapes, a title, an explanation, an optional button.
ColumnLayout {
    id: root

    property string icon: "music_note"
    property string title
    property string text
    property string actionText
    property string actionIcon
    property var shape: "cookie9"
    property bool animated: true

    signal action()

    spacing: 12

    // The shapes slowly morph into one another: a morph every few seconds, and between morphs
    // the window is not redrawn. Behind the main shape — a pale larger one, phase-shifted.
    Item {
        id: art
        Layout.alignment: Qt.AlignHCenter
        Layout.preferredWidth: 156
        Layout.preferredHeight: 156
        Layout.bottomMargin: 4

        readonly property var cycle: [root.shape, "cookie12", "flower8", "clover4", "sunny", "cookie6", "flower6", "cookie9"]
        property int step: 0

        Timer {
            interval: 2800
            repeat: true
            running: root.visible && root.animated && !Theme.quiet && Theme.reducedMotion !== true
            onTriggered: art.step++
        }

        MorphShape {
            anchors.centerIn: parent
            width: 156
            height: 156
            shape: art.cycle[(art.step + 3) % art.cycle.length]
            duration: Theme.motion.spatialSlow * 2
            overshoot: 1.05
            color: Theme.primaryContainer
            opacity: 0.35
            rotation: -art.step * 25
            Behavior on rotation { NumberAnimation { duration: Theme.motion.spatialSlow * 2; easing.type: Easing.InOutCubic } }
        }
        MorphShape {
            anchors.centerIn: parent
            width: 120
            height: 120
            shape: art.cycle[art.step % art.cycle.length]
            duration: Theme.motion.spatialSlow * 1.6
            overshoot: 1.15
            color: Theme.secondaryContainer
            rotation: art.step * 40
            Behavior on rotation { NumberAnimation { duration: Theme.motion.spatialSlow * 1.6; easing.type: Easing.InOutCubic } }
        }
        Icon {
            anchors.centerIn: parent
            name: root.icon
            size: 46
            fill: 1
            color: Theme.fgSecondaryContainer
        }
    }

    Label {
        Layout.alignment: Qt.AlignHCenter
        Layout.fillWidth: true
        Layout.maximumWidth: 420
        text: root.title
        wrapMode: Text.WordWrap
        elide: Text.ElideNone
        type: "titleLarge"
        horizontalAlignment: Text.AlignHCenter
    }
    Label {
        visible: root.text !== ""
        Layout.alignment: Qt.AlignHCenter
        Layout.fillWidth: true
        Layout.maximumWidth: 420
        text: root.text
        type: "bodyMedium"
        color: Theme.fgSurfaceVariant
        horizontalAlignment: Text.AlignHCenter
        wrapMode: Text.WordWrap
        elide: Text.ElideNone
        lineHeightMode: Text.ProportionalHeight
        lineHeight: 1.25
    }
    Button {
        visible: root.actionText !== ""
        Layout.alignment: Qt.AlignHCenter
        Layout.topMargin: 8
        text: root.actionText
        icon: root.actionIcon
        style: "tonal"
        onClicked: root.action()
    }
}
