pragma Singleton
import QtQuick

// Global limit on concurrent cover loads. Dozens of MorphImages on one page request
// decoding at once — QQuickPixmapDecoder chokes the GUI thread and opening the page pins 100% of a core.
// The queue lets twelve through: decoding stretches a couple of frames longer, but the window stays responsive.
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

    // the item must call release when loading finishes (Ready/Error/Null) or the source changes to ""
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

    // Something waiting in the queue goes away (source reset / element destroyed): it leaves the queue without
    // returning a slot — no slot was issued to it, and release() would wrongly decrement the counter (an extra parallel decode).
    function cancel(item) {
        const i = root._queue.indexOf(item)
        if (i >= 0)
            root._queue.splice(i, 1)
    }
}
