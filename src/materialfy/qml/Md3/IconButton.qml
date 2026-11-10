import QtQuick

// MD3 Expressive icon button: style standard | filled | tonal | outlined,
// size xs | s | m | l | xl, width narrow | default | wide. Supports toggle.
Item {
    id: root

    property string icon
    property string style: "standard"
    property string size: "s"
    property string widthMode: "default"
    property bool checkable: false
    property bool checked: false
    // false — the state is only set from outside (checked: binding), a click just emits clicked()
    property bool autoToggle: true
    property color iconColor: _contentColor
    // Small glyphs (the three dots more_horiz) are too faint at the standard size and weight: they get lost
    property real iconSize: _spec.icon
    property real iconWeight: 400
    readonly property alias pressed: area.pressed
    readonly property alias hovered: area.containsMouse

    signal clicked()

    readonly property var _spec: ({
        xs: { h: 32,  icon: 20, narrow: 28,  wide: 40,  pressedR: 8,  squareR: 12 },
        s:  { h: 40,  icon: 24, narrow: 32,  wide: 52,  pressedR: 8,  squareR: 12 },
        m:  { h: 56,  icon: 24, narrow: 48,  wide: 72,  pressedR: 12, squareR: 16 },
        l:  { h: 96,  icon: 32, narrow: 64,  wide: 128, pressedR: 16, squareR: 28 },
        xl: { h: 136, icon: 40, narrow: 104, wide: 184, pressedR: 16, squareR: 28 }
    })[size]

    readonly property bool _selected: checkable && checked

    readonly property color containerColor: {
        if (!enabled)
            return style === "standard" || style === "outlined" ? "transparent" : Qt.alpha(Theme.fgSurface, Theme.stateLayer.disabledContainer)
        switch (style) {
        case "filled": return checkable && !checked ? Theme.surfaceContainer : Theme.primary
        case "tonal": return checkable && !checked ? Theme.surfaceContainerHighest : Theme.secondaryContainer
        case "outlined": return _selected ? Theme.inverseSurface : "transparent"
        // A selected standard button — with a tonal backing: the icon color alone is not enough,
        // with an unlucky palette primary is barely different from fgSurfaceVariant.
        default: return _selected ? Theme.secondaryContainer : "transparent"
        }
    }
    readonly property color _contentColor: {
        if (!enabled)
            return Qt.alpha(Theme.fgSurface, Theme.stateLayer.disabledContent)
        switch (style) {
        case "filled": return checkable && !checked ? Theme.fgSurfaceVariant : Theme.fgPrimary
        case "tonal": return checkable && !checked ? Theme.fgSurfaceVariant : Theme.fgSecondaryContainer
        case "outlined": return _selected ? Theme.fgInverseSurface : Theme.fgSurfaceVariant
        default: return _selected ? Theme.fgSecondaryContainer : Theme.fgSurfaceVariant
        }
    }

    property real cornerRadius: area.pressed ? _spec.pressedR : _selected ? _spec.squareR : height / 2
    Behavior on cornerRadius {
        NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutBack; easing.overshoot: Theme.motion.overshoot }
    }

    implicitHeight: _spec.h
    implicitWidth: widthMode === "narrow" ? _spec.narrow : widthMode === "wide" ? _spec.wide : _spec.h

    Rectangle {
        id: container
        anchors.fill: parent
        radius: Math.min(root.cornerRadius, height / 2)
        color: root.containerColor
        border.width: root.style === "outlined" && !root._selected ? 1 : 0
        border.color: Theme.outlineVariant
        Behavior on color { enabled: !Theme.transitioning; ColorAnimation { duration: Theme.motion.effectsDefault } }
    }

    Icon {
        anchors.centerIn: parent
        name: root.icon
        size: root.iconSize
        weight: root.iconWeight
        fill: root._selected ? 1 : 0
        color: root.iconColor
    }

    StateLayer {
        id: area
        anchors.fill: parent
        enabled: root.enabled
        radius: container.radius
        color: root.iconColor
        onClicked: {
            if (root.checkable && root.autoToggle)
                root.checked = !root.checked
            root.clicked()
        }
    }
}
