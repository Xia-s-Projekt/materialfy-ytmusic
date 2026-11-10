import QtQuick

// MD3 Expressive slider: a thick track, a bar handle with gaps, a stop dot,
// and a popup value while dragging.
Item {
    id: root

    property real from: 0
    property real to: 1
    property real value: 0
    property real stepSize: 0
    property real trackHeight: 16
    property real handleHeight: 44
    property real gap: 6
    property bool showValue: true
    // the wheel changes the value; false — the wheel scrolls the page (sliders in long settings lists)
    property bool wheelEnabled: true
    property var valueText: v => Math.round(v * 100) + "%"
    property color activeColor: Theme.primary
    property color inactiveColor: Theme.secondaryContainer
    property color handleColor: Theme.primary

    readonly property alias pressed: area.pressed
    // The value while dragging/wheeling: show it instead of value so that value is not
    // written imperatively — otherwise the value binding is lost forever, and
    // two sliders on one Player.volume stop synchronizing (they write without reading).
    // A NaN sentinel: ranges can be negative (-500..500 for Sync), so a sign check will not do
    property real _live: NaN
    property bool _liveActive: false
    readonly property real position: {
        const v = _liveActive ? _live : value
        return to === from ? 0 : (v - from) / (to - from)
    }

    signal moved(real value)

    implicitWidth: 240
    implicitHeight: handleHeight

    function _setFromX(x) {
        let p = Math.max(0, Math.min(1, (x - _usableX0) / (_usableX1 - _usableX0)))
        let v = from + p * (to - from)
        if (stepSize > 0)
            v = from + Math.round((v - from) / stepSize) * stepSize
        if (_liveActive === false || v !== _live) {
            _live = v
            _liveActive = true
            moved(v)
        }
    }

    property real _handleW: area.pressed ? 2 : 4
    Behavior on _handleW { NumberAnimation { duration: Theme.motion.effectsFast } }
    readonly property real _usableX0: _handleW / 2
    readonly property real _usableX1: width - _handleW / 2
    readonly property real _handleX: _usableX0 + (_usableX1 - _usableX0) * position
    readonly property real _outer: trackHeight / 2
    readonly property real _inner: 2

    // Active part
    Rectangle {
        readonly property real w: root._handleX - root._handleW / 2 - root.gap
        visible: w > 0
        x: 0
        width: Math.max(w, 0)
        height: root.trackHeight
        anchors.verticalCenter: parent.verticalCenter
        color: root.activeColor
        topLeftRadius: Math.min(root._outer, width / 2)
        bottomLeftRadius: Math.min(root._outer, width / 2)
        topRightRadius: Math.min(root._inner, width / 2)
        bottomRightRadius: Math.min(root._inner, width / 2)
    }

    // Inactive part
    Rectangle {
        id: inactive
        readonly property real x0: root._handleX + root._handleW / 2 + root.gap
        visible: root.width - x0 > 0
        x: x0
        width: Math.max(root.width - x0, 0)
        height: root.trackHeight
        anchors.verticalCenter: parent.verticalCenter
        color: root.inactiveColor
        topRightRadius: Math.min(root._outer, width / 2)
        bottomRightRadius: Math.min(root._outer, width / 2)
        topLeftRadius: Math.min(root._inner, width / 2)
        bottomLeftRadius: Math.min(root._inner, width / 2)

        Rectangle {
            width: 4
            height: 4
            radius: 2
            anchors.verticalCenter: parent.verticalCenter
            anchors.right: parent.right
            anchors.rightMargin: (root.trackHeight - 4) / 2
            color: root.activeColor
            visible: parent.width > root.trackHeight
        }
    }

    // Handle
    Rectangle {
        id: handle
        width: root._handleW
        height: root.handleHeight
        radius: width / 2
        x: root._handleX - width / 2
        anchors.verticalCenter: parent.verticalCenter
        color: root.handleColor
    }

    // Value above the handle
    Rectangle {
        visible: opacity > 0
        opacity: root.showValue && area.pressed ? 1 : 0
        scale: opacity
        transformOrigin: Item.Bottom
        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }
        width: Math.max(48, valueLabel.implicitWidth + 24)
        height: 44
        radius: height / 2
        color: Theme.inverseSurface
        x: handle.x + handle.width / 2 - width / 2
        y: handle.y - height - 4

        Label {
            id: valueLabel
            anchors.centerIn: parent
            type: "labelLarge"
            color: Theme.fgInverseSurface
            text: root.valueText(root._liveActive ? root._live : root.value)
        }
    }

    MouseArea {
        id: area
        anchors.fill: parent
        cursorShape: Qt.PointingHandCursor
        preventStealing: true
        onPressed: mouse => root._setFromX(mouse.x)
        onPositionChanged: mouse => { if (pressed) root._setFromX(mouse.x) }
        onReleased: root._liveActive = false
        onCanceled: root._liveActive = false
        onWheel: wheel => {
            if (!root.wheelEnabled) {
                wheel.accepted = false
                return
            }
            const step = root.stepSize > 0 ? root.stepSize : (root.to - root.from) / 50
            // The base is the live value, not a binding: with a lagging property (remote volume), quick
            // wheel steps within a 150 ms window would all be computed from the same stale value and be lost
            const base = root._liveActive ? root._live : root.value
            const v = Math.max(root.from, Math.min(root.to, base + (wheel.angleDelta.y > 0 ? step : -step)))
            if (root._liveActive === false || v !== root._live) {
                root._live = v
                root._liveActive = true
                root.moved(v)
            }
            // the wheel sends no release: live is held until the value binding catches up
            wheelLive.restart()
        }
    }

    Timer {
        id: wheelLive
        interval: 150
        onTriggered: root._liveActive = false
    }
}
