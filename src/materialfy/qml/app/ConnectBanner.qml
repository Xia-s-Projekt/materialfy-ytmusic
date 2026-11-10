import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Remote control badge: "Playing on {name} · This computer works as a remote · Listen here" (primary 18 %).
// The lost variant (the device is gone) — errorContainer, "Continue from m:ss on this computer?".
// It reads Player/Devices itself and hides itself when music plays here.
Item {
    id: root

    property string variant: Player.deviceLost === true ? "lost" : "normal"   // "normal" | "lost"
    readonly property bool lost: variant === "lost"
    readonly property bool shown: Player.hasTrack && (Player.remote === true || Player.deviceLost === true)
    // this computer cannot play yet — instead of transferring, we lead to the playback settings
    readonly property bool needsSetup: Player.localState === "unavailable" || Player.localState === "failed"

    property string _name: ""
    readonly property string deviceName: (Player.device ?? {}).name ?? ""
    onDeviceNameChanged: if (deviceName !== "") _name = deviceName
    Component.onCompleted: if (deviceName !== "") _name = deviceName

    DeviceBadge.Info { id: info; type: (Player.device ?? {}).type ?? "" }

    function _time(ms) {
        const s = Math.max(0, Math.round(ms / 1000))
        return Math.floor(s / 60) + ":" + String(s % 60).padStart(2, "0")
    }
    function _listenHere() {
        if (needsSetup)
            Catalog.openRequested("playback", {})
        else if (lost)
            Player.transferHere()
        else
            Devices.transferHere()
    }

    readonly property string title: {
        const n = _name !== "" ? _name : I18n.t("Another device")
        return lost ? I18n.t("{0} stopped responding", n) : I18n.t("Playing on {0}", n)
    }
    readonly property string subtitle: lost ? I18n.t("Continue from {0} on this computer?", _time(Player.positionMs))
                                     : Player.playing ? I18n.t("This computer is acting as a remote") : I18n.t("Paused · remote")

    implicitWidth: 420
    implicitHeight: shown ? pill.implicitHeight : 0
    visible: implicitHeight > 0.5
    opacity: shown ? 1 : 0
    clip: true
    Behavior on implicitHeight {
        enabled: !(Theme.reducedMotion === true)
        NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized }
    }
    Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }

    // normal ↔ lost: the colour transitions, the text cross-fades
    onLostChanged: if (visible && !(Theme.reducedMotion === true)) textSwap.restart()
    SequentialAnimation {
        id: textSwap
        NumberAnimation { target: texts; property: "opacity"; to: 0; duration: Theme.motion.effectsFast }
        NumberAnimation { target: texts; property: "opacity"; to: 1; duration: Theme.motion.effectsDefault }
    }

    // tight spot (the queue's right panel): the button moves under the text, the text wraps
    readonly property bool stacked: width < 400

    Rectangle {
        id: pill
        width: parent.width
        implicitHeight: Math.max(52, row.implicitHeight + (root.stacked ? 24 : 12))
        height: implicitHeight
        radius: root.stacked ? Theme.shape.large : Theme.shape.extraLarge
        color: root.lost ? Theme.errorContainer : Qt.alpha(Theme.primary, 0.18)
        Behavior on color { enabled: !Theme.transitioning; ColorAnimation { duration: Theme.motion.effectsDefault } }

        GridLayout {
            id: row
            anchors.fill: parent
            anchors.leftMargin: root.stacked ? 12 : 8
            anchors.rightMargin: root.stacked ? 12 : 6
            anchors.topMargin: root.stacked ? 12 : 0
            anchors.bottomMargin: root.stacked ? 12 : 0
            columns: root.stacked ? 2 : 3
            columnSpacing: 12
            rowSpacing: 10

            MorphShape {
                Layout.preferredWidth: 40
                Layout.preferredHeight: 40
                Layout.row: 0
                Layout.column: 0
                Layout.alignment: root.stacked ? Qt.AlignTop : Qt.AlignVCenter
                shape: "softSquare"
                color: root.lost ? Theme.fgErrorContainer : Theme.primary
                Icon {
                    anchors.centerIn: parent
                    name: root.lost ? "portable_wifi_off" : info.icon
                    size: 22
                    fill: 1
                    weight: 500
                    color: root.lost ? Theme.errorContainer : Theme.fgPrimary
                }
            }

            ColumnLayout {
                id: texts
                Layout.fillWidth: true
                Layout.row: 0
                Layout.column: 1
                spacing: 0
                Label {
                    Layout.fillWidth: true
                    text: root.title
                    type: "titleSmall"
                    color: root.lost ? Theme.fgErrorContainer : Theme.fgSurface
                }
                Label {
                    Layout.fillWidth: true
                    text: root.subtitle
                    type: "bodySmall"
                    wrapMode: root.stacked ? Text.WordWrap : Text.NoWrap
                    lineHeightMode: Text.ProportionalHeight
                    lineHeight: 1.15
                    color: root.lost ? Theme.fgErrorContainer : Theme.fgSurfaceVariant
                }
            }

            // Button: tonal in the normal state, filled with the badge's text colour in lost
            Item {
                id: action
                readonly property bool moving: Player.transferring === true
                Layout.preferredWidth: actionRow.implicitWidth + 32
                Layout.preferredHeight: 40
                Layout.column: root.stacked ? 1 : 2
                Layout.row: root.stacked ? 1 : 0
                enabled: !moving

                Rectangle {
                    anchors.fill: parent
                    radius: actionHit.pressed ? 12 : height / 2
                    color: root.lost ? Theme.fgErrorContainer : Theme.secondaryContainer
                    Behavior on radius { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutBack } }
                    Behavior on color { enabled: !Theme.transitioning; ColorAnimation { duration: Theme.motion.effectsDefault } }
                }
                Row {
                    id: actionRow
                    anchors.centerIn: parent
                    spacing: 6
                    LoadingIndicator {
                        running: !Theme.calm
                        visible: action.moving
                        anchors.verticalCenter: parent.verticalCenter
                        size: 20
                        color: root.lost ? Theme.errorContainer : Theme.fgSecondaryContainer
                    }
                    Icon {
                        visible: !action.moving
                        anchors.verticalCenter: parent.verticalCenter
                        tight: true
                        name: root.needsSetup ? "speaker" : "computer"
                        size: 20
                        color: root.lost ? Theme.errorContainer : Theme.fgSecondaryContainer
                    }
                    Label {
                        anchors.verticalCenter: parent.verticalCenter
                        text: action.moving ? I18n.t("Transferring…") : root.needsSetup ? I18n.t("Set up") : I18n.t("Play here")
                        type: "labelLarge"
                        color: root.lost ? Theme.errorContainer : Theme.fgSecondaryContainer
                    }
                }
                StateLayer {
                    id: actionHit
                    anchors.fill: parent
                    radius: height / 2
                    color: root.lost ? Theme.errorContainer : Theme.fgSecondaryContainer
                    onClicked: root._listenHere()
                }
            }
        }
    }
}
