import QtQuick

// Edges of the frameless window: they pull startSystemResize (under tiling the compositor simply ignores it).
Item {
    id: root

    required property Window window
    property int grip: 6

    anchors.fill: parent
    visible: window.visibility === Window.Windowed
    z: 1000

    component Edge: MouseArea {
        required property int edges
        hoverEnabled: true
        acceptedButtons: Qt.LeftButton
        cursorShape: edges === (Qt.LeftEdge | Qt.TopEdge) || edges === (Qt.RightEdge | Qt.BottomEdge) ? Qt.SizeFDiagCursor
                   : edges === (Qt.RightEdge | Qt.TopEdge) || edges === (Qt.LeftEdge | Qt.BottomEdge) ? Qt.SizeBDiagCursor
                   : edges === Qt.LeftEdge || edges === Qt.RightEdge ? Qt.SizeHorCursor : Qt.SizeVerCursor
        onPressed: root.window.startSystemResize(edges)
    }

    Edge { edges: Qt.LeftEdge; x: 0; y: root.grip; width: root.grip; height: root.height - 2 * root.grip }
    Edge { edges: Qt.RightEdge; x: root.width - root.grip; y: root.grip; width: root.grip; height: root.height - 2 * root.grip }
    Edge { edges: Qt.TopEdge; x: root.grip; y: 0; width: root.width - 2 * root.grip; height: root.grip }
    Edge { edges: Qt.BottomEdge; x: root.grip; y: root.height - root.grip; width: root.width - 2 * root.grip; height: root.grip }
    Edge { edges: Qt.LeftEdge | Qt.TopEdge; x: 0; y: 0; width: root.grip * 2; height: root.grip * 2 }
    Edge { edges: Qt.RightEdge | Qt.TopEdge; x: root.width - root.grip * 2; y: 0; width: root.grip * 2; height: root.grip * 2 }
    Edge { edges: Qt.LeftEdge | Qt.BottomEdge; x: 0; y: root.height - root.grip * 2; width: root.grip * 2; height: root.grip * 2 }
    Edge { edges: Qt.RightEdge | Qt.BottomEdge; x: root.width - root.grip * 2; y: root.height - root.grip * 2; width: root.grip * 2; height: root.grip * 2 }
}
