import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Top content bar: back, scrolled page title, page buttons, window buttons.
// Empty space drags the window (startSystemMove), double click — maximize.
Item {
    id: root

    required property var router
    required property Window window
    // narrow window: the side panel is hidden in a sheet, this button opens it
    property bool showMenuButton: false
    signal menuClicked()

    implicitHeight: 64

    readonly property var page: router.currentItem
    // the title of a page with its own header (heroTitle: true) appears only after scrolling
    readonly property bool showTitle: page && (page.scrolled || !(page.showHeader || page.heroTitle === true)) && (page.title ?? "") !== ""

    // Window dragging. Windows: startSystemMove works when the window has a system menu
    // (the shell's native frame restores it). If the system refuses, we move the window ourselves.
    DragHandler {
        property bool manual: false
        target: null
        grabPermissions: PointerHandler.TakeOverForbidden
        onActiveChanged: if (active) manual = !root.window.startSystemMove()
        onCentroidChanged: if (active && manual && root.window.visibility === Window.Windowed) {
            root.window.x += centroid.scenePosition.x - centroid.scenePressPosition.x
            root.window.y += centroid.scenePosition.y - centroid.scenePressPosition.y
        }
    }
    TapHandler {
        acceptedButtons: Qt.LeftButton
        onDoubleTapped: windowButtons.toggleMaximized()
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 16
        anchors.rightMargin: 16
        spacing: 8

        // the sheet button slides in and pushes the title aside instead of popping in
        Item {
            Layout.preferredWidth: root.showMenuButton ? menuButton.implicitWidth : 0
            Layout.rightMargin: root.showMenuButton ? 0 : -8
            Layout.preferredHeight: menuButton.implicitHeight
            Behavior on Layout.preferredWidth { NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
            Behavior on Layout.rightMargin { NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
            clip: true
            IconButton {
                id: menuButton
                icon: "menu"
                enabled: root.showMenuButton
                opacity: root.showMenuButton ? 1 : 0
                scale: root.showMenuButton ? 1 : 0.6
                Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }
                Behavior on scale { NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.OutBack } }
                onClicked: root.menuClicked()
            }
        }

        // Without "back" the button collapses and the title sits at the left edge
        Item {
            Layout.preferredWidth: back.enabled ? back.implicitWidth : 0
            // the title aligns to the left edge of the page content (32 px margin)
            Layout.rightMargin: back.enabled || root.showMenuButton ? 0 : 8
            Layout.preferredHeight: back.implicitHeight
            Behavior on Layout.preferredWidth { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutCubic } }
            Behavior on Layout.rightMargin { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutCubic } }
            clip: true
            IconButton {
                id: back
                icon: "arrow_back"
                enabled: root.router.depth > 1
                opacity: enabled ? 1 : 0
                Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
                onClicked: root.router.back()
            }
        }

        Label {
            Layout.fillWidth: true
            text: root.page ? root.page.title : ""
            type: "titleLarge"
            weight: 500
            opacity: root.showTitle ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }
        }

        // Page buttons (library search, sort, view) — page.toolbar: Component
        Loader {
            Layout.alignment: Qt.AlignVCenter
            active: !!root.page && !!root.page.toolbar
            sourceComponent: root.page ? root.page.toolbar ?? null : null
        }

        WindowButtons {
            id: windowButtons
            window: root.window
        }
    }
}
