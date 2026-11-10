import QtQuick
import Md3

// Row of settings section chips. In a narrow window it scrolls horizontally;
// the selected chip scrolls itself into the visible area.
Item {
    id: root

    property var sections: []          // [{ key, text, icon }]
    property string current
    signal picked(string key)

    implicitHeight: 48
    implicitWidth: row.implicitWidth

    Flickable {
        id: flick
        anchors.fill: parent
        contentWidth: row.implicitWidth
        contentHeight: height
        flickableDirection: Flickable.HorizontalFlick
        boundsBehavior: Flickable.StopAtBounds
        interactive: contentWidth > width
        clip: true

        Behavior on contentX {
            enabled: !flick.moving
            NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized }
        }

        Row {
            id: row
            anchors.verticalCenter: parent.verticalCenter
            spacing: 8
            Repeater {
                id: chips
                model: root.sections
                Chip {
                    required property var modelData
                    text: modelData.text
                    icon: modelData.icon ?? ""
                    selected: root.current === modelData.key
                    onClicked: root.picked(modelData.key)
                }
            }
        }

        // the mouse wheel turns the row horizontally, only if it does not fit
        WheelHandler {
            enabled: flick.interactive
            acceptedDevices: PointerDevice.Mouse
            onWheel: e => {
                flick.contentX = Math.max(0, Math.min(flick.contentWidth - flick.width, flick.contentX - e.angleDelta.y))
                e.accepted = true
            }
        }
    }

    onCurrentChanged: {
        const i = sections.findIndex(s => s.key === current)
        const chip = i >= 0 ? chips.itemAt(i) : null
        if (!chip || !flick.interactive)
            return
        if (chip.x < flick.contentX)
            flick.contentX = Math.max(0, chip.x - 16)
        else if (chip.x + chip.width > flick.contentX + flick.width)
            flick.contentX = Math.min(flick.contentWidth - flick.width, chip.x + chip.width - flick.width + 16)
    }
}
