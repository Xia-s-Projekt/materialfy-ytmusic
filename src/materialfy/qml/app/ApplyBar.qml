import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Bar "New settings will apply after a restart… · Apply" (secondaryContainer).
// Shown by Settings.engineRestartPending; it appears and leaves by collapsing its height.
Item {
    id: root

    property bool shown: Settings.engineRestartPending === true
    property string text: I18n.t("New settings apply after playback restarts; the music will pause for a second")
    signal applied()

    readonly property bool _instant: Theme.reducedMotion === true
    implicitWidth: 480
    implicitHeight: shown ? bar.implicitHeight : 0
    visible: height > 0.5
    clip: true
    opacity: shown ? 1 : 0
    Behavior on implicitHeight {
        enabled: !root._instant
        NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized }
    }
    Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }

    Rectangle {
        id: bar
        width: parent.width
        implicitHeight: Math.max(56, row.implicitHeight + 16)
        height: implicitHeight
        radius: Theme.shape.large
        color: Theme.secondaryContainer

        RowLayout {
            id: row
            anchors.fill: parent
            anchors.leftMargin: 16
            anchors.rightMargin: 8
            spacing: 12
            Icon { name: "restart_alt"; size: 22; color: Theme.fgSecondaryContainer }
            Label {
                Layout.fillWidth: true
                text: root.text
                type: "bodyMedium"
                color: Theme.fgSecondaryContainer
                wrapMode: Text.WordWrap
                elide: Text.ElideNone
                maximumLineCount: 3
            }
            Button {
                text: I18n.t("Apply")
                onClicked: {
                    Settings.applyEngineSettings()
                    root.applied()
                }
            }
        }
    }
}
