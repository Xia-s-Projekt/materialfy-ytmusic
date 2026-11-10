import QtQuick
import Md3
import Materialfy.Core

// The "Devices" tab of the right panel (like the Connect side panel in Spotify for desktop):
// a DeviceList across the full height of the panel. The list refreshes when the tab becomes visible.
SmoothFlickable {
    id: root

    property bool shown: false

    onShownChanged: if (shown) Devices.refresh()
    Component.onCompleted: if (shown) Devices.refresh()

    wheelStep: Settings.wheelStep
    contentWidth: width
    contentHeight: body.implicitHeight + 16
    interactive: contentHeight > height

    DeviceList {
        id: body
        width: root.width
        onCloseRequested: Settings.rightPanelOpen = false
    }
}
