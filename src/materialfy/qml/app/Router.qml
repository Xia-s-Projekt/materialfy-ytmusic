import QtQuick
import Md3

// Page stack. Live page instances are kept in the stack, so "back" preserves scroll and state.
// MD3 transitions: between sections — fade through, deeper/back — shared axis X.
Item {
    id: root

    // page name → component url
    property var pages: ({})
    property var stack: []                 // [{ name, props, item }]
    readonly property int depth: stack.length
    readonly property var current: stack.length ? stack[stack.length - 1] : null
    readonly property string currentName: current ? current.name : ""
    readonly property string rootName: stack.length ? stack[0].name : ""
    readonly property Item currentItem: current ? current.item : null

    // Pages that live between visits. "home" — without parameters (the home page is heavy — hundreds
    // of covers, losing the scroll and recreating it on every return is expensive). keepAliveSlots — how many
    // pages with parameters (playlists, artists) to keep alive on an LRU basis: reopening
    // the same playlist back and forth is instant and preserves the scroll, without rebuilding rows.
    property var keepAlive: ["home"]
    // They are built synchronously on first entry (the start screen); the other keepAlive ones — asynchronously.
    property var keepAliveSync: ["home"]
    property int keepAliveSlots: 6
    property var _kept: ({})                // name → live instance from keepAlive (no parameters)
    property var _keptKeyed: []             // LRU: [{ key, name, item }], oldest at the end

    property var _components: ({})
    // Page compilation warm-up: the first open of each page compiles its QML chain
    // synchronously on the GUI thread — a freeze on the first open. We compile one component per tick
    // after startup, while the user is on the home page; on real navigation we pause and warm up after it.
    // One tick = one page: the home scroll does not jitter.
    property bool _prewarmed: false
    // Order: first compile the heavy pages (playlist, library), then the quiet build
    // of the ones that live forever (settings/liked/...): their instances are reused immediately on entry.
    // Parameterized ones (playlist, artist) cannot be incubated without props — we keep only the compilation.
    function _prewarm() {
        if (_prewarmed)
            return
        _prewarmed = true
        const names = Object.keys(pages).filter(n => keepAlive.indexOf(n) < 0)
        prewarmTimer.names = names.concat(keepAlive.filter(n => n !== "home" && _kept[n] === undefined))
        prewarmTimer.at = 0
        prewarmTimer.restart()
    }
    Timer {
        id: prewarmTimer
        interval: 80
        repeat: true
        property var names: []
        property int at: 0
        onTriggered: {
            if (_pending !== null) {
                restart()
                return
            }
            if (at >= names.length) {
                stop()
                return
            }
            const name = names[at++]
            let comp = _components[name]
            if (!comp) {
                comp = Qt.createComponent(pages[name])
                _components[name] = comp
            }
            if (comp.status === Component.Error) {
                console.error("Router prewarm:", comp.errorString())
                return
            }
            // Parameterized pages cannot be built (props are needed): the compilation itself is the warm-up.
            if (keepAlive.indexOf(name) < 0)
                return
            if (_kept[name] !== undefined)
                return
            // FIX 4: quiet build via async incubation — a synchronous createObject built
            // a full page in a single frame (a freeze of hundreds of ms on Home).
            const incubator = comp.incubateObject(
                container,
                { router: root, width: container.width, height: container.height, opacity: 0 },
                Qt.Asynchronous)
            incubator.onStatusChanged = function() {
                if (incubator.status !== Component.Ready)
                    return
                const item = incubator.object
                if (!item) {
                    console.error("Router prewarm:", comp.errorString())
                    return
                }
                if (_kept[name] !== undefined && _kept[name] !== item) {
                    item.destroy()      // the user opened the page sooner — the instance from _make already lives
                    return
                }
                item.width = Qt.binding(() => container.width)
                item.height = Qt.binding(() => container.height)
                item.visible = false
                _kept[name] = item
            }
        }
    }
    property var _leaving: []               // items to destroy after the animation
    property Item _out: null
    property var _pending: null             // incomplete incubation: { name, props, comp, incubator, prev, kind }

    function _same(entry, name, props) {
        return entry && entry.name === name && JSON.stringify(entry.props ?? {}) === JSON.stringify(props ?? {})
    }

    // A repeated request for the same page while it is still being built is not a new push, but the same thing
    function _pendingSame(name, props) {
        return _pending !== null && _pending.name === name && JSON.stringify(_pending.props) === JSON.stringify(props ?? {})
    }

    function push(name, props) {
        if (_same(current, name, props))
            return
        if (_pending && _pending.name === name && JSON.stringify(_pending.props) === JSON.stringify(props ?? {})) {
            if (_pending.kind !== "prewarm")
                return                          // the same request is already being built — not a new push
            // A click on a card we are quietly warming up: the warm-up becomes a transition
            _pending.kind = "forward"
            _pending.prev = currentItem
            return
        }
        if (_pending)
            _cancelPending()                // push A → push B: cancel the unfinished A and build B
        const prev = currentItem
        const entry = _make(name, props, "forward")
        if (!entry)
            return                          // incubation is already running (or the error is already logged), the continuation is in _onIncubated
        stack = stack.concat([entry])
        _animate(prev, entry.item, "forward")
    }

    // Navigation to a top-level section: the stack is reset.
    function reset(name, props) {
        // a quiet warm-up of the same page does not count — we cancel it and do a normal reset
        if ((_pendingSame(name, props) && _pending.kind !== "prewarm") || (depth === 1 && _same(current, name, props)))
            return
        if (_pending)
            _cancelPending()                // reset rewrites the stack anyway — the incubation is no longer needed
        const prev = currentItem
        const entry = _make(name, props, "fade")
        if (!entry)
            return
        _leaving = _leaving.concat(stack.map(e => e.item).filter(i => i !== prev))
        if (prev)
            _leaving = _leaving.concat([prev])
        stack = [entry]
        _animate(prev, entry.item, "fade")
    }

    function back() {
        if (_pending !== null) {
            // The page has not reached the stack yet: canceling the incubation is the "back", the stack is left alone.
            // The user never saw the quiet warm-up — canceling it does not swallow the "back"
            const silent = _pending.kind === "prewarm"
            _cancelPending()
            if (!silent)
                return true
        }
        if (depth < 2)
            return false
        const popped = stack[stack.length - 1]
        stack = stack.slice(0, -1)
        _leaving = _leaving.concat([popped.item])
        const next = currentItem
        next.visible = true
        _animate(popped.item, next, "back")
        return true
    }

    // A null return with a live _pending is not an error, but "building asynchronously"; kind is needed by _onIncubated
    function _make(name, props, kind) {
        const url = pages[name]
        if (!url) {
            console.error("Router: no such page", name)
            return null
        }
        let comp = _components[name]
        if (!comp) {
            comp = Qt.createComponent(url)
            _components[name] = comp
        }
        if (comp.status === Component.Error) {
            console.error("Router:", comp.errorString())
            return null
        }
        const keep = keepAlive.indexOf(name) >= 0 && Object.keys(props ?? {}).length === 0
        const kept = keep ? _kept[name] : null
        if (kept && !stack.some(e => e.item === kept)) {
            kept.opacity = 0
            return { name: name, props: {}, item: kept }
        }
        // keepAlive without a live instance: home — synchronously (startup), the others asynchronously —
        // the single build must not freeze, after that the page lives forever
        // Pages with parameters: key = name + props. A live instance with the same key is reused
        // (an instant return, scroll in place); otherwise we build by incubation and put it in the LRU.
        const key = name + "|" + JSON.stringify(props ?? {})
        const keptKeyed = _takeKeptKeyed(key)
        if (keptKeyed && !stack.some(e => e.item === keptKeyed)) {
            keptKeyed.opacity = 0
            return { name: name, props: props ?? {}, item: keptKeyed }
        }
        // Size — immediately on creation: a page created at zero width at once "grew" to its real size,
        // and the Behavior of the header (cover, margins), the album column on rows and the layout animated for half a second
        // and relaid out on every frame of the transition. The same applies to incubation — the initial width/height
        // and opacity 0, so the unfinished page does not flash, and the binding is attached only at Ready.
        if (keep && keepAliveSync.indexOf(name) >= 0) {
            const item = comp.createObject(container, Object.assign({ router: root, width: container.width, height: container.height },
                                                                    props ?? {}))
            if (!kept)
                _kept[name] = item
            // not anchors.fill: transition animations move x
            item.width = Qt.binding(() => container.width)
            item.height = Qt.binding(() => container.height)
            item.opacity = 0
            return { name: name, props: props ?? {}, item: item }
        }
        _incubate(comp, name, props, kind)
        return null
    }

    function _incubate(comp, name, props, kind) {
        const incubator = comp.incubateObject(
            container,
            Object.assign({ router: root, width: container.width, height: container.height }, props ?? {}, { opacity: 0 }),
            Qt.Asynchronous)
        _pending = { name: name, props: props ?? {}, comp: comp, incubator: incubator, prev: currentItem, kind: kind, keep: keepAlive.indexOf(name) >= 0 && Object.keys(props ?? {}).length === 0 }
        incubator.onStatusChanged = function() { _onIncubated(incubator) }
    }

    function _onIncubated(incubator) {
        const p = _pending
        if (p === null || p.incubator !== incubator) {
            // A stale request (push/reset/back changed): the unfinished item is destroyed,
            // so it does not pile up in the container at opacity 0 and keep loading.
            if (incubator.status === Component.Ready && incubator.object)
                incubator.object.destroy()
            return
        }
        if (incubator.status === Component.Loading)
            return
        _pending = null
        if (incubator.status !== Component.Ready) {
            console.error("Router:", p.comp.errorString())
            return
        }
        let item = incubator.object
        // There is no frame with a different size: before Ready the page is transparent, and the binding is attached in the same frame as the animation
        item.width = Qt.binding(() => container.width)
        item.height = Qt.binding(() => container.height)
        item.opacity = 0
        if (p.keep && _kept[p.name] !== undefined && _kept[p.name] !== item) {
            // prewarm got there first: its instance is already live, the one created here goes to waste
            item.destroy()
            item = _kept[p.name]
        } else if (p.keep && _kept[p.name] === undefined) {
            _kept[p.name] = item
        }
        if (p.kind === "forward" && stack.some(e => e.item === item)) {
            // The page is already open deeper in the stack (the fresh copy was burned above): the duplicate is not added to the stack
            return
        }
        // LRU — only for pages with parameters (playlists, artists): search/account/playback
        // must not survive navigation (state resets on entry), without props they are not cached
        if (keepAlive.indexOf(p.name) < 0 && Object.keys(p.props).length > 0)
            _rememberKeyed(p, item)
        const entry = { name: p.name, props: p.props, item: item }
        if (p.kind === "prewarm") {
            // Quiet warm-up: the page is hidden in the LRU, with no stack and no animation.
            item.visible = false
            item.z = 0
            return
        }
        if (p.kind === "forward") {
            stack = stack.concat([entry])
            _animate(p.prev, item, "forward")
        } else {
            _leaving = _leaving.concat(stack.map(e => e.item).filter(i => i !== p.prev))
            if (p.prev)
                _leaving = _leaving.concat([p.prev])
            stack = [entry]
            _animate(p.prev, item, "fade")
        }
    }

    // Hovering a card: the page is built after a pause (dwell) — the cursor on its way to the target
    // touches a dozen tiles, each with its own props, and without the pause the incubations overflow
    // the queue. hover-out cancels the scheduled warm-up.
    function prewarmHover(name, props) {
        hoverPrewarm.name = name
        hoverPrewarm.props = props
        hoverPrewarm.restart()
    }
    function prewarmHoverCancel(name, props) {
        // a shared timer: cancel only your own warm-up, otherwise the A→B transition would kill B's warm-up
        if (hoverPrewarm.name === name && JSON.stringify(hoverPrewarm.props) === JSON.stringify(props ?? {}))
            hoverPrewarm.stop()
    }
    Timer {
        id: hoverPrewarm
        interval: 200
        property string name: ""
        property var props: null
        onTriggered: prewarmKeyed(name, props)
    }

    // Warming up a parameterized page (hovering a card): we build by incubation without a push,
    // the result goes straight into the LRU. A click on the card finds the live instance — the transition is instant.
    function prewarmKeyed(name, props) {
        if (_components[name] === undefined)
            return
        if (_pendingSame(name, props) || stack.some(e => _same(e, name, props)))
            return
        const key = name + "|" + JSON.stringify(props ?? {})
        if (_hasKeptKeyed(key))
            return
        if (_pending !== null)
            return                          // we do not interrupt a real transition
        const comp = _components[name] || Qt.createComponent(pages[name])
        if (comp.status === Component.Error || comp.status === Component.Loading)
            return
        _components[name] = comp
        _incubate(comp, name, props, "prewarm")
    }

    function _hasKeptKeyed(key) {
        return _keptKeyed.some(e => e.key === key)
    }

    function _takeKeptKeyed(key) {
        for (let i = 0; i < _keptKeyed.length; i++) {
            if (_keptKeyed[i].key === key) {
                const e = _keptKeyed[i]
                _keptKeyed.splice(i, 1)
                _keptKeyed.push(e)          // LRU: the most recent goes to the end
                return e.item
            }
        }
        return null
    }

    function _rememberKeyed(p, item) {
        const key = p.name + "|" + JSON.stringify(p.props)
        if (_takeKeptKeyed(key))
            return                          // already in the LRU (should not happen, but safe)
        _keptKeyed.push({ key: key, name: p.name, item: item })
        while (_keptKeyed.length > keepAliveSlots) {
            const evicted = _keptKeyed.shift()
            if (!stack.some(e => e.item === evicted.item))
                evicted.item.destroy()
        }
    }

    function _cancelPending() {
        const p = _pending
        if (p === null)
            return
        // The JS incubator has no cancel(): we mark the request stale. _onIncubated will see
        // p !== _pending and destroy the unfinished object itself, once the status reaches Ready.
        _pending = null
        if (p.incubator.status === Component.Ready && p.incubator.object)
            p.incubator.object.destroy()
    }

    function _animate(from, to, kind) {
        if (anim.running)
            anim.complete()
        _out = from
        from = from ?? dummy
    // Sections (fade through): the new page rises from below and slightly "grows" to size.
    // Deeper/back (shared axis X): the pages slide horizontally toward each other.
        const axis = kind === "fade" ? "y" : "x"
        const d = kind === "forward" ? 140 : kind === "back" ? -140 : 72
        outFade.target = from
        outMove.target = from
        outMove.property = axis
        outMove.to = kind === "fade" ? -24 : -d / 2
        outScale.target = from
        outScale.to = kind === "fade" ? 0.97 : 1
        inFade.target = to
        inMove.target = to
        inMove.property = axis
        inMove.from = d
        inScale.target = to
        inScale.from = kind === "fade" ? 0.94 : 1
        to.x = 0
        to.y = 0
        to.visible = true
        to.z = 1
        from.z = 0
        anim.start()
        // Software rendering: the page changes at once. Each of the ~35 transition frames is drawn by the CPU
        // in full, together with the just-created page — that is the "hang" on opening
        if (Theme.lowPower)
            anim.complete()
    }

    Item {
        id: container
        anchors.fill: parent
    }

    Item { id: dummy; visible: false }

    // A departed page leaves tens of MB of garbage in the JS heap (its models, bindings): the collector reaches it
    // not soon, and the process memory stays occupied all that time. A full collection — one second after the transition,
    // once the animation has finished (5–25 ms), and only if the pages really were destroyed.
    Timer { id: gcTimer; interval: 1000; onTriggered: gc() }

    ParallelAnimation {
        id: anim

        NumberAnimation { id: outFade; property: "opacity"; to: 0; duration: 160; easing.type: Easing.OutCubic }
        NumberAnimation {
            id: outMove; duration: 300
            easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedAccelerate
        }
        NumberAnimation { id: outScale; property: "scale"; duration: 300; easing.type: Easing.OutCubic }
        SequentialAnimation {
            PauseAnimation { duration: 60 }
            ParallelAnimation {
                NumberAnimation { id: inFade; property: "opacity"; from: 0; to: 1; duration: 260; easing.type: Easing.OutCubic }
                NumberAnimation {
                    id: inMove; to: 0; duration: 560
                    easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedDecelerate
                }
                NumberAnimation {
                    id: inScale; property: "scale"; to: 1; duration: 560
                    easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedDecelerate
                }
            }
        }

        onFinished: {
            if (root._out) {
                root._out.visible = false
                root._out.x = 0
                root._out.y = 0
                root._out.scale = 1
            }
            const kept = Object.values(root._kept).concat(root._keptKeyed.map(e => e.item))
            for (const item of root._leaving) {
                if (kept.indexOf(item) < 0) {
                    item.destroy()
                    gcTimer.restart()
                } else if (item !== root.currentItem) {
                    item.visible = false
                    item.x = 0
                    item.y = 0
                    item.scale = 1
                }
            }
            root._leaving = []
            root._out = null
        }
    }
}
