import QtQuick

// MD3 container surface (the reference's floating panels).
Rectangle {
    property string level: "container"   // lowest | low | container | high | highest | surface
    property int elevation: 0
    property real translucency: 0          // 0..1 — the window background shows through the surface

    radius: Theme.shape.extraLarge
    color: Qt.alpha(_base, 1 - translucency)
    readonly property color _base: ({
        lowest: Theme.surfaceContainerLowest,
        low: Theme.surfaceContainerLow,
        container: Theme.surfaceContainer,
        high: Theme.surfaceContainerHigh,
        highest: Theme.surfaceContainerHighest,
        surface: Theme.surface
    })[level] ?? Theme.surfaceContainer
}
