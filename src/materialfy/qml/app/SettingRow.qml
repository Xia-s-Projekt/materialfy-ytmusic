import QtQuick
import QtQuick.Layouts
import Md3

// Settings row: title + description on the left, control on the right (placed inside).
// If the control does not fit next to the label (narrow window) — it moves under it.
GridLayout {
    id: root

    property string title
    property string description
    default property alias control: slot.data

    Layout.fillWidth: true
    // the width is set by the parent: otherwise the implicit width of the two columns inflates the row and it does not wrap
    Layout.preferredWidth: 0
    // the control wraps under the label with a margin of ±16 px, otherwise the row would flicker at the boundary while stretching
    property bool _stacked: false
    function _updateStack() {
        if (width <= 0)
            return
        const edge = slot.implicitWidth + 200
        _stacked = _stacked ? width < edge + 16 : width < edge - 16
    }
    onWidthChanged: _updateStack()
    Component.onCompleted: _stacked = width > 0 && width < slot.implicitWidth + 200
    columns: _stacked ? 1 : 2
    columnSpacing: 24
    rowSpacing: 10

    ColumnLayout {
        Layout.fillWidth: true
        spacing: 2
        Label { Layout.fillWidth: true; text: root.title; type: "bodyLarge" }
        Label {
            visible: root.description !== ""
            Layout.fillWidth: true
            text: root.description
            type: "bodySmall"
            color: Theme.fgSurfaceVariant
            wrapMode: Text.WordWrap
            elide: Text.ElideNone
            lineHeightMode: Text.ProportionalHeight
            lineHeight: 1.2
        }
    }
    Item {
        id: slot
        implicitWidth: childrenRect.width
        implicitHeight: childrenRect.height
        // the control became wider (another language, a longer button label) — recalculate the wrap
        onImplicitWidthChanged: root._updateStack()
    }
}
