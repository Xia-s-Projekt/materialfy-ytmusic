import QtQuick

// Connected button group MD3 Expressive (like Normal | Circular in the reference).
// model: ["Normal", "Circular"] or [{ text, icon }]; currentIndex is the selected segment.
Row {
    id: root

    property var model: []
    property int currentIndex: 0
    property string size: "s"
    property bool showCheck: false
    // false — currentIndex is only set from outside (binding), a click only emits activated()
    property bool autoSelect: true
    property color segmentColor: Theme.surfaceContainerHighest   // unselected segment

    signal activated(int index)

    spacing: 2

    Repeater {
        model: root.model

        delegate: Item {
            id: seg

            required property var modelData
            required property int index
            readonly property bool selected: index === root.currentIndex
            readonly property bool first: index === 0
            readonly property bool last: index === root.model.length - 1
            readonly property string label: typeof modelData === "string" ? modelData : (modelData.text ?? "")
            readonly property string iconName: typeof modelData === "string" ? "" : (modelData.icon ?? "")
            readonly property var spec: ({ xs: { h: 32, pad: 12, inner: 4 }, s: { h: 40, pad: 16, inner: 8 }, m: { h: 56, pad: 24, inner: 8 } })[root.size]

            // The selected segment and the outer corners are fully round, the inner ones have a small radius.
            readonly property real full: height / 2
            property real leftR: selected || first ? full : (area.pressed ? spec.inner / 2 : spec.inner)
            property real rightR: selected || last ? full : (area.pressed ? spec.inner / 2 : spec.inner)
            Behavior on leftR { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutBack } }
            Behavior on rightR { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutBack } }

            implicitHeight: spec.h
            implicitWidth: content.implicitWidth + 2 * spec.pad

            Rectangle {
                id: bg
                anchors.fill: parent
                color: seg.selected ? Theme.secondaryContainer : root.segmentColor
                topLeftRadius: seg.leftR
                bottomLeftRadius: seg.leftR
                topRightRadius: seg.rightR
                bottomRightRadius: seg.rightR
                Behavior on color { enabled: !Theme.transitioning; ColorAnimation { duration: Theme.motion.effectsDefault } }
            }

            Row {
                id: content
                anchors.centerIn: parent
                spacing: 6
                Icon {
                    tight: true
                    readonly property string n: seg.iconName !== "" ? seg.iconName : (root.showCheck && seg.selected ? "check" : "")
                    visible: n !== ""
                    name: n
                    size: 18
                    fill: seg.selected ? 1 : 0
                    color: seg.selected ? Theme.fgSecondaryContainer : Theme.fgSurfaceVariant
                    anchors.verticalCenter: parent.verticalCenter
                }
                Label {
                    visible: seg.label !== ""
                    text: seg.label
                    type: "labelLarge"
                    color: seg.selected ? Theme.fgSecondaryContainer : Theme.fgSurfaceVariant
                    anchors.verticalCenter: parent.verticalCenter
                }
            }

            StateLayer {
                id: area
                anchors.fill: parent
                color: seg.selected ? Theme.fgSecondaryContainer : Theme.fgSurfaceVariant
                topLeftRadius: seg.leftR
                bottomLeftRadius: seg.leftR
                topRightRadius: seg.rightR
                bottomRightRadius: seg.rightR
                onClicked: {
                    if (root.autoSelect)
                        root.currentIndex = seg.index
                    root.activated(seg.index)
                }
            }
        }
    }
}
