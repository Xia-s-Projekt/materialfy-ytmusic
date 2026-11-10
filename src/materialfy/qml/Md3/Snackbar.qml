import QtQuick

// A short MD3 notification at the bottom center; it hides itself. Placed over the window:
// Snackbar { id: snackbar; anchors.fill: parent; z: … }; snackbar.show("Link copied").
// With an action: show("Track removed", "Undo", () => …) — a button on the right, shown for longer.
Item {
    id: root

    property real bottomMargin: 112   // above the mini player
    property int duration: 2500
    property int actionDuration: 5000
    property bool shown: false
    property string actionText
    property var _onAction: null

    signal action()

    // holdMs — how long to keep it up (optional; defaults to duration or actionDuration)
    function show(text, actionText, onAction, holdMs) {
        label.text = text
        root.actionText = actionText ?? ""
        _onAction = typeof onAction === "function" ? onAction : null
        shown = true
        hideTimer.interval = holdMs > 0 ? holdMs : root.actionText !== "" ? actionDuration : duration
        hideTimer.restart()
    }
    function hide() { shown = false }

    Timer { id: hideTimer; interval: root.duration; onTriggered: root.hide() }

    Rectangle {
        id: pill
        anchors.horizontalCenter: parent.horizontalCenter
        y: root.height - root.bottomMargin - height + (root.shown ? 0 : 16)
        width: Math.min(label.implicitWidth + 48 + (actionButton.visible ? actionButton.implicitWidth + 8 : 0), root.width - 32)
        height: 48
        radius: root.actionText !== "" ? Theme.shape.medium : height / 2
        color: Theme.inverseSurface
        opacity: root.shown ? 1 : 0
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }
        Behavior on y { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutCubic } }

        Elevation { target: pill; level: 3 }

        // a click on the banner does not reach the content beneath it
        MouseArea { anchors.fill: parent; acceptedButtons: Qt.AllButtons }

        Label {
            id: label
            x: actionButton.visible ? 16 : (parent.width - width) / 2
            anchors.verticalCenter: parent.verticalCenter
            width: Math.min(implicitWidth, parent.width - 48 - (actionButton.visible ? actionButton.implicitWidth : 0))
            elide: Text.ElideRight
            type: "bodyMedium"
            color: Theme.fgInverseSurface
        }

        Item {
            id: actionButton
            visible: root.actionText !== ""
            anchors.right: parent.right
            anchors.rightMargin: 4
            anchors.verticalCenter: parent.verticalCenter
            implicitWidth: actionLabel.implicitWidth + 24
            width: implicitWidth
            height: 40
            Label {
                id: actionLabel
                anchors.centerIn: parent
                text: root.actionText
                type: "labelLarge"
                color: Theme.inversePrimary
            }
            StateLayer {
                anchors.fill: parent
                radius: height / 2
                color: Theme.inversePrimary
                onClicked: {
                    const f = root._onAction
                    root._onAction = null
                    root.hide()
                    root.action()
                    if (f)
                        f()
                }
            }
        }
    }
}
