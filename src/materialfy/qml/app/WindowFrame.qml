import QtQuick
import Md3

// Window frame without system chrome. It takes over all window content (Component.onCompleted) and draws
// the window background under it.
// Windows: DWM rounds the corners (winframe.cpp, Windows 11), here — background only.
// Linux / macOS: the window is transparent (Main.qml: color), the app is drawn as a rounded rectangle of radius
// Theme.shape.large — as a layer masked with roundclip; for a maximized and fullscreen window the corners are square. Transparent
// corners fall under the ResizeEdges corner zones: a press in them drags the window corner. The shadow is whatever the system gives.
Item {
    id: root

    required property Window window
    // the window draws its own edge itself (Linux / macOS, not maximized)
    readonly property bool framed: Qt.platform.os !== "windows" && window.visibility === Window.Windowed
    // On software rendering the corners are square: a layer over the whole window is an extra pass over every pixel in every frame,
    // and changing content in a "live" layer also doubles the frames (Qt asks for the next one after every change)
    readonly property bool rounded: framed && !Theme.lowPower
    readonly property real radius: rounded ? Theme.shape.large : 0
    // the layer is enabled only after the content has been moved: otherwise the layer effect (placed next to it,
    // in contentItem) would end up inside the frame itself
    property bool _adopted: false

    anchors.fill: parent

    Rectangle {
        anchors.fill: parent
        color: Theme.surface
    }

    layer.enabled: rounded && _adopted
    layer.effect: ShaderEffect {
        readonly property real dpr: root.Screen.devicePixelRatio > 0 ? root.Screen.devicePixelRatio : 1
        property size pixelSize: Qt.size(width * dpr, height * dpr)
        property real radius: root.radius * dpr
        fragmentShader: Qt.resolvedUrl("shaders/roundclip.frag.qsb")
    }

    // Thin outline: the Linux compositor often gives a frameless window neither shadow nor edge
    Rectangle {
        anchors.fill: parent
        z: 100000
        visible: root.framed
        radius: root.radius
        color: "transparent"
        border.width: 1
        border.color: Qt.alpha(Theme.outlineVariant, 0.6)
    }

    Component.onCompleted: {
        for (const child of [...window.contentItem.children]) {
            if (child !== root)
                child.parent = root
        }
        _adopted = true
    }
}
