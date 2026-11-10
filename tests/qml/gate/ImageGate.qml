pragma Singleton
import QtQuick

// Global limit on simultaneous cover loads. Dozens of MorphImage on one page request
// decoding at once — QQuickPixmapDecoder chokes the GUI thread and opening the page pegs a core at 100 %.
// The queue lets through twelve at a time: decoding stretches a couple of frames longer, but the window stays responsive.
QtObject {
    id: root

    property int running: 0
    readonly property int limit: 12
    property var _queue: []

    function acquire(item) {
        if (root.running < root.limit) {
            root.running++
            return true
        }
        root._queue.push(item)
        return false
    }

    // the item must call release when loading has finished (Ready/Error/Null) or the source has changed to ""
    function release(item) {
        root.running = Math.max(0, root.running - 1)
        while (root._queue.length > 0 && root.running < root.limit) {
            const next = root._queue.shift()
            if (next !== null && next.qmlContinue !== undefined) {
                root.running++
                next.qmlContinue()
            }
        }
    }

    // A queued waiter leaves (source reset / element destroyed): removed from the queue without returning a slot —
    // no slot was given to it, and release() would wrongly drop the counter (an extra parallel decode).
    function cancel(item) {
        const i = root._queue.indexOf(item)
        if (i >= 0)
            root._queue.splice(i, 1)
    }
}
