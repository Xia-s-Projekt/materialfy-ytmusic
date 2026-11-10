import QtQuick
import QtQuick.Layouts
import Md3

// A banner inside a settings card: error — errorContainer (no Premium), info — secondaryContainer.
// It appears by height and opacity (shown), not instantly.
Item {
    id: root

    property string kind: "info"      // info | error
    property string icon: kind === "error" ? "block" : "info"
    property string title
    property string text
    property string actionText
    property string actionIcon
    property bool busy: false
    property bool shown: true

    signal action()

    readonly property color _bg: kind === "error" ? Theme.errorContainer : Theme.secondaryContainer
    readonly property color _fg: kind === "error" ? Theme.fgErrorContainer : Theme.fgSecondaryContainer

    Layout.fillWidth: true
    implicitWidth: 320
    implicitHeight: shown ? body.implicitHeight + 32 : 0
    Behavior on implicitHeight { NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
    opacity: shown ? 1 : 0
    Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }
    visible: implicitHeight > 0.5
    clip: true

    Rectangle {
        anchors.fill: parent
        radius: Theme.shape.largeIncreased
        color: root._bg
    }

    RowLayout {
        id: body
        x: 16
        y: 16
        width: root.width - 32
        spacing: 14

        Icon {
            Layout.alignment: Qt.AlignTop
            name: root.icon
            size: 22
            fill: 1
            color: root._fg
        }
        ColumnLayout {
            Layout.fillWidth: true
            spacing: 2
            Label {
                visible: root.title !== ""
                Layout.fillWidth: true
                text: root.title
                type: "titleSmall"
                color: root._fg
                wrapMode: Text.WordWrap
                elide: Text.ElideNone
            }
            Label {
                visible: root.text !== ""
                Layout.fillWidth: true
                text: root.text
                type: "bodyMedium"
                color: root._fg
                wrapMode: Text.WordWrap
                elide: Text.ElideNone
            }
        }
        LoadingIndicator {
            visible: root.busy
            size: 28
            color: root._fg
        }
        Button {
            visible: root.actionText !== "" && !root.busy
            text: root.actionText
            icon: root.actionIcon
            style: root.kind === "error" ? "outlined" : "filled"
            size: "xs"
            onClicked: root.action()
        }
    }
}
