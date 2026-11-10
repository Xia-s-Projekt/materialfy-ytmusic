// ImageGate slot accounting ( Md3/ImageGate.qml — the real singleton, not a copy):
//   QT_QPA_PLATFORM=offscreen /usr/lib/qt6/bin/qmltestrunner -input tests/qml/tst_imagegate.qml
// (tests/qml/gate/ImageGate.qml — a copy of the real singleton: symlinks do not work on Windows
//  (core.symlinks=false), and the whole Md3 module cannot come here — its singletons pull in Materialfy.Core.
//  When the original changes the copy must be updated: they go in one commit.)
// Contract: acquire up to the limit — a slot immediately; beyond that — the queue; release wakes a waiter via
// qmlContinue; cancel removes a waiter without returning a slot (no slot was given to it);
// an extra release does not drive the counter negative; a dead request is skipped.
import QtQuick
import QtTest
import "gate" as Gate

TestCase {
    name: "ImageGate"

    // fake "Image": the queue calls qmlContinue on whoever it put in the queue
    Component {
        id: fakeFactory
        QtObject {
            property int continues: 0
            function qmlContinue() { continues += 1 }
        }
    }

    // take all the slots up to the limit; return the holders that were given
    function fillToLimit() {
        const holders = []
        for (let i = 0; i < Gate.ImageGate.limit; ++i) {
            const o = createTemporaryObject(fakeFactory, null)
            compare(Gate.ImageGate.acquire(o), true, "slot " + i + " given immediately")
            holders.push(o)
        }
        return holders
    }

    function drainQueue() {
        // singleton state between tests: release the slots that were given, drain the queue
        while (Gate.ImageGate.running > 0)
            Gate.ImageGate.release(null)
        Gate.ImageGate._queue = []
        compare(Gate.ImageGate.running, 0)
        compare(Gate.ImageGate._queue.length, 0)
    }

    function test_acquire_up_to_limit_then_queue() {
        drainQueue()
        const holders = fillToLimit()
        const waiter = createTemporaryObject(fakeFactory, null)
        compare(Gate.ImageGate.acquire(waiter), false, "over the limit — queued")
        compare(Gate.ImageGate._queue.length, 1)
        for (const h of holders)
            Gate.ImageGate.release(h)
        compare(waiter.continues, 1, "the waiter got a slot via qmlContinue")
        compare(Gate.ImageGate.running, 1)
        drainQueue()
    }

    function test_release_wakes_queue_in_order() {
        drainQueue()
        const holders = fillToLimit()
        const w1 = createTemporaryObject(fakeFactory, null)
        const w2 = createTemporaryObject(fakeFactory, null)
        compare(Gate.ImageGate.acquire(w1), false)
        compare(Gate.ImageGate.acquire(w2), false)
        Gate.ImageGate.release(holders[0])
        compare(w1.continues, 1, "the first waiter is woken first")
        compare(w2.continues, 0, "the second is waiting for its slot")
        compare(Gate.ImageGate.running, Gate.ImageGate.limit)
        Gate.ImageGate.release(w1)
        compare(w2.continues, 1, "the freed slot goes to the next one")
        drainQueue()
    }

    function test_cancel_does_not_return_slot() {
        drainQueue()
        const holders = fillToLimit()
        const waiter = createTemporaryObject(fakeFactory, null)
        compare(Gate.ImageGate.acquire(waiter), false)
        Gate.ImageGate.cancel(waiter)         // left without getting a slot
        compare(Gate.ImageGate._queue.length, 0)
        Gate.ImageGate.release(holders[0])    // the only slot was returned
        compare(waiter.continues, 0, "the cancelled one is not woken")
        compare(Gate.ImageGate.running, Gate.ImageGate.limit - 1, "the slot did not go to the cancelled one")
        drainQueue()
    }

    function test_release_below_zero_is_clamped() {
        drainQueue()
        const o = createTemporaryObject(fakeFactory, null)
        Gate.ImageGate.release(o)             // release without acquire
        compare(Gate.ImageGate.running, 0, "the counter did not go negative")
        const w = createTemporaryObject(fakeFactory, null)
        compare(Gate.ImageGate.acquire(w), true, "after the extra release the limit is not consumed")
        drainQueue()
    }

    function test_queued_item_without_continue_is_skipped() {
        drainQueue()
        const holders = fillToLimit()
        Gate.ImageGate._queue.push(null)      // a "dead" request in the queue
        const w = createTemporaryObject(fakeFactory, null)
        compare(Gate.ImageGate.acquire(w), false)
        Gate.ImageGate.release(holders[0])
        compare(w.continues, 1, "the dead request was skipped, the live one woken")
        drainQueue()
    }
}
