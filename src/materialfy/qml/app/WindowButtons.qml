import QtQuick
import Md3
import Materialfy.Core

// MD3 window buttons: minimize, maximize / restore, close. The application draws the window frame itself, so
// the buttons exist on every OS and in every layout (the mini player has its own, in the MiniMode pill).
// A fullscreen window (F11) is returned to normal by the "restore" button.
Row {
    id: root

    required property Window window
    readonly property bool expanded: !!window && (window.visibility === Window.Maximized || window.visibility === Window.FullScreen)

    // and for the double click on the empty space of the top bar
    function toggleMaximized() {
        window.visibility = expanded ? Window.Windowed : Window.Maximized
    }

    spacing: 4

    component WindowButton: IconButton {
        id: button
        property string label
        size: "xs"
        Accessible.role: Accessible.Button
        Accessible.name: label
        Accessible.onPressAction: button.clicked()
    }

    WindowButton {
        icon: "remove"
        label: I18n.t("Minimize")
        // not showMinimized(): that clears "maximized", and restoring from the taskbar would give a normal window
        onClicked: WindowShell.minimize(root.window)
    }
    WindowButton {
        icon: root.expanded ? "collapse_content" : "expand_content"
        label: root.expanded ? I18n.t("Restore down") : I18n.t("Maximize")
        onClicked: root.toggleMaximized()
    }
    WindowButton {
        icon: "close"
        label: I18n.t("Close")
        // hover tints "close" with the error color — like the red button of a system frame, but in theme tones
        iconColor: hovered ? Theme.error : Theme.fgSurfaceVariant
        onClicked: root.window.close()
    }
}
