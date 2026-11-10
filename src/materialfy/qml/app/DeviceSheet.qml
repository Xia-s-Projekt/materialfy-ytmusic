import QtQuick
import QtQuick.Templates as T
import Md3
import Materialfy.Core

// Device sheet at the bottom for a narrow window (layoutMode === "compact"): scrim 32 %, surfaceContainerLow,
// radius 28 at the top, handle; content — DeviceList, like in the right panel's tab, actions — as buttons.
// Closes on the scrim, Esc, and a downward swipe of the handle.
T.Popup {
    id: root

    readonly property bool _instant: Theme.reducedMotion === true

    onClosed: sheet.drag = 0

    function openSheet() {
        Devices.refresh()
        open()
    }

    parent: T.Overlay.overlay
    x: 0
    y: 0
    width: parent ? parent.width : 0
    height: parent ? parent.height : 0
    padding: 0
    modal: true
    dim: false
    focus: true
    closePolicy: T.Popup.CloseOnEscape
    background: null

    // opening: the scrim fades in, the sheet slides up from the bottom
    property real reveal: 0
    enter: Transition {
        NumberAnimation {
            property: "reveal"; from: 0; to: 1
            duration: root._instant ? 0 : Theme.motion.spatialDefault
            easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedDecelerate
        }
    }
    exit: Transition {
        NumberAnimation { property: "reveal"; to: 0; duration: Theme.motion.effectsDefault; easing.type: Easing.InCubic }
    }

    contentItem: Item {
        Rectangle {
            anchors.fill: parent
            color: Theme.scrim
            opacity: 0.32 * Math.min(1, root.reveal * 1.5)
            MouseArea {
                anchors.fill: parent
                acceptedButtons: Qt.AllButtons
                onClicked: root.close()
                onWheel: wheel => wheel.accepted = true
            }
        }

        Rectangle {
            id: sheet
            property real drag: 0
            anchors.horizontalCenter: parent.horizontalCenter
            width: Math.min(parent.width, 640)
            height: Math.min(handleBox.height + flick.contentHeight + 16, parent.height * 0.85)
            y: parent.height - height * (root._instant ? 1 : root.reveal) + drag + (root._instant ? 0 : (1 - root.reveal) * 40)
            opacity: root._instant ? root.reveal : Math.min(1, root.reveal * 2)
            topLeftRadius: Theme.shape.extraLarge
            topRightRadius: Theme.shape.extraLarge
            color: Theme.surfaceContainerLow
            Behavior on drag { enabled: !dragHandler.active; NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutCubic } }

            // a press on empty sheet space (labels, fields) must not pass through to the scrim under it and close the sheet
            MouseArea {
                anchors.fill: parent
                acceptedButtons: Qt.AllButtons
                onWheel: wheel => wheel.accepted = true
            }

            // handle: drag down — close
            Item {
                id: handleBox
                width: parent.width
                height: 28
                Rectangle {
                    anchors.centerIn: parent
                    width: 32
                    height: 4
                    radius: 2
                    color: Theme.outline
                    opacity: 0.6
                }
                DragHandler {
                    id: dragHandler
                    target: null
                    yAxis.enabled: true
                    xAxis.enabled: false
                    onTranslationChanged: sheet.drag = Math.max(0, translation.y)
                    onActiveChanged: if (!active) {
                        if (sheet.drag > 80)
                            root.close()
                        else
                            sheet.drag = 0
                    }
                }
            }

            Flickable {
                id: flick
                anchors.top: handleBox.bottom
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.bottom: parent.bottom
                anchors.leftMargin: 12
                anchors.rightMargin: 12
                anchors.bottomMargin: 16
                clip: true
                contentWidth: width
                contentHeight: body.implicitHeight
                boundsBehavior: Flickable.StopAtBounds
                interactive: contentHeight > height

                DeviceList {
                    id: body
                    width: flick.width
                    touch: true
                    onCloseRequested: root.close()
                }
            }
        }
    }
}
