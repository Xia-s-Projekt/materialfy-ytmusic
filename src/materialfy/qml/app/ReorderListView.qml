import QtQuick
import Md3

// ListView with row reordering by a drag handle (playlist edit mode). The delegate calls beginDrag(index),
// updateDrag(dy, sceneY), endDrag() and shifts by shiftFor(index); the lifted row travels by dragOffset.
// Neighbors make room for the row height (a Behavior in the delegate), and near the edges the list scrolls itself.
// Release → moveRequested(from, to) (to is the resulting row index). Until the model answers, the rows stay in their
// new places; once the model has moved the rows (or nothing changed for 1.5 s) the shifts drop without animation.
ListView {
    id: view

    property bool reorderable: false
    property real rowHeight: 64
    readonly property bool dragActive: dragFrom >= 0 && !_settling
    property int dragFrom: -1
    property int dragTo: -1
    property real _dy: 0
    property real _scrollStart: 0
    property real _pointerY: 0             // in list coordinates
    property bool _settling: false         // released: waiting for the model to move the rows
    property bool shiftAnimated: true      // neighbors shift smoothly (false — while the shifts drop)

    signal moveRequested(int from, int to)

    readonly property real dragOffset: dragFrom < 0 ? 0
        : _settling ? (dragTo - dragFrom) * rowHeight
        : _dy + (contentY - _scrollStart)

    function shiftFor(index) {
        if (dragFrom < 0 || index === dragFrom)
            return 0
        if (dragFrom < dragTo && index > dragFrom && index <= dragTo)
            return -rowHeight
        if (dragFrom > dragTo && index >= dragTo && index < dragFrom)
            return rowHeight
        return 0
    }

    function beginDrag(index) {
        if (!reorderable || _settling)
            return
        dragFrom = index
        dragTo = index
        _dy = 0
        _scrollStart = contentY
    }
    function updateDrag(dy, sceneY) {
        if (dragFrom < 0 || _settling)
            return
        _dy = dy
        _pointerY = view.mapFromItem(null, 0, sceneY).y
        _retarget()
    }
    function _retarget() {
        const n = count
        dragTo = Math.max(0, Math.min(n - 1, dragFrom + Math.round(dragOffset / rowHeight)))
    }
    function endDrag() {
        if (dragFrom < 0 || _settling)
            return
        if (dragTo !== dragFrom) {
            _settling = true
            settleTimeout.restart()
            moveRequested(dragFrom, dragTo)
        } else {
            dragFrom = -1
            dragTo = -1
        }
    }
    function _settle() {
        settleTimeout.stop()
        shiftAnimated = false
        _settling = false
        dragFrom = -1
        dragTo = -1
        Qt.callLater(() => view.shiftAnimated = true)
    }
    Timer {
        id: settleTimeout
        interval: 1500
        onTriggered: {
            // no answer from the model — the rows smoothly return (the service does the rollback)
            view._settling = false
            view.dragFrom = -1
            view.dragTo = -1
        }
    }
    Connections {
        target: view.model
        ignoreUnknownSignals: true
        enabled: view._settling
        function onRowsMoved() { view._settle() }
        function onModelReset() { view._settle() }
        function onDataChanged() { view._settle() }
        function onRowsInserted() { view._settle() }
        function onRowsRemoved() { view._settle() }
        function onLayoutChanged() { view._settle() }
    }

    // Auto-scroll near the top and bottom edge, only while the row is lifted
    Timer {
        interval: 16
        repeat: true
        running: view.dragActive && (view._pointerY < 56 || view._pointerY > view.height - 56)
        onTriggered: {
            const edge = 56
            const v = view._pointerY < edge ? -(edge - view._pointerY) / edge : (view._pointerY - (view.height - edge)) / edge
            const maxY = view.originY + Math.max(0, view.contentHeight - view.height)
            view.contentY = Math.max(view.originY, Math.min(maxY, view.contentY + v * 14))
            view._retarget()
        }
    }
}
