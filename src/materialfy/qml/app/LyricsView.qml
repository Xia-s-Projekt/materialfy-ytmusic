import QtQuick
import QtQuick.Effects
import Md3
import Materialfy.Core

// Synced lyrics for the playing track with word-by-word fill (a port of the Word Lyrics widget for DMS).
// The active line is held at anchorPos; when the line changes the text moves in a cascade:
// lines below are touched slightly later. Inactive lines are dimmed and blurred, pauses are "• • •".
// Clicking a line seeks to it. Wheel, trackpad and dragging scroll the text manually:
// line following pauses and resumes after resumeMs or via the "To current line" button.
Item {
    id: root

    // The base size is set by the call site (full player, side panel); the "Text size" setting
    // scales it. Everything else (line height, margins, pause shapes) is derived from fontSize.
    property real baseFontSize: 34
    property real fontSize: baseFontSize * Settings.lyricsFontScale
    property real anchorPos: spicy ? 0.36 : 0.33
    // Backdrop under the text: "none" — transparent; "soft" | "strong" — a dim in the theme color
    readonly property string scrimMode: Settings.lyricsScrim
    readonly property real scrimAlpha: scrimMode === "strong" ? 0.6 : (scrimMode === "soft" ? 0.28 : 0)

    // Display style:
    // "normal" — the classic look: cascade, blurred neighboring lines, left alignment;
    // "spicy" — an expressive word-by-word wave with a wide gradient fill and a soft glow;
    // "karaoke" — karaoke: a large line with a crisp word fill and smooth transitions between lines.
    readonly property string mode: Settings.lyricsMode
    readonly property bool spicy: mode === "spicy"
    readonly property bool karaoke: mode === "karaoke"

    // Appearance and behavior parameters: settings override the style defaults
    readonly property string align: Settings.lyricsTextAlign
    readonly property real blurAmount: Settings.lyricsBlurAmount
    readonly property bool blurInactive: blurAmount > 0.001
    readonly property string linesMode: Settings.lyricsVisibleLines
    readonly property string fillMode: Settings.lyricsFillMode
    readonly property bool motion: Settings.lyricsAnimate && !Theme.reducedMotion

    // Manual settings for line accent, word wave amplitude and the opacity of neighboring lines
    // (with safe fallback defaults, in case the Settings bridge is built without the new keys).
    readonly property real userActiveScale: Settings.lyricsActiveScale ?? 0
    readonly property real activeScale: userActiveScale >= 1.0
        ? userActiveScale
        : (spicy ? 1.12 : (karaoke ? 1.16 : 1.0))
    readonly property real baseIdleScale: spicy ? 0.88 : (karaoke ? 0.90 : 0.94)
    readonly property real idleScale: Settings.lyricsIdleScale > 0
        ? Math.min(activeScale, Settings.lyricsIdleScale)
        : baseIdleScale
    readonly property real wordBounce: Settings.lyricsWordBounce ?? 1.0
    readonly property real dimAlpha: Settings.lyricsDimOpacity ?? (spicy ? 0.34 : 0.35)
    readonly property real baseSpacingScale: spicy ? 1.12 : (karaoke ? 1.08 : 1.0)
    readonly property real spacingScale: baseSpacingScale * Settings.lyricsLineSpacing
    readonly property real highlightMultiplier: Settings.lyricsHighlight
    property bool showSource: Settings.lyricsShowSource

    // Number of visible lines around the active one: "one"/"1" → 0, "three"/"3" → 1, "all"/"15" → all,
    // or any number N from the "Visible lines" slider (shows N lines around the current one).
    readonly property int maxVisibleDist: {
        const s = String(linesMode ?? "all").trim()
        if (s === "all" || s === "")
            return 999
        if (s === "one")
            return 0
        if (s === "three")
            return 1
        const n = parseInt(s, 10)
        if (!isNaN(n) && n >= 1)
            return n >= 15 ? 999 : Math.floor((n - 1) / 2)
        return 999
    }

    property real fadeTopPct: 0.22
    property real fadeBottomPct: 0.18
    property int scrollDuration: !Settings.lyricsAnimate ? 0 : (Settings.lyricsScrollMs >= 0 ? Settings.lyricsScrollMs : (spicy ? 420 : (karaoke ? 440 : 600)))
    property int scrollStagger: !Settings.lyricsAnimate ? 0 : (Settings.lyricsStaggerMs >= 0 ? Settings.lyricsStaggerMs : (spicy ? 28 : (karaoke ? 26 : 42)))
    // Smooth scroll lead: the line gently travels to anchorPos and grows toward the start of the first note,
    // while the highlight and fill follow the synced audio clock exactly (see tick()).
    property int scrollLeadMs: 240
    readonly property int scrollAheadMs: !Settings.lyricsAnimate ? 0 : (spicy ? 240 : (karaoke ? 250 : Math.max(scrollLeadMs, Math.round(scrollDuration * 0.45))))
    property color textColor: Theme.fgSurface
    property int resumeMs: 3000

    // The preview in settings — the same look, but with its own lines and clock; the normal path is unchanged.
    property bool previewMode: false
    property var previewLines: []
    property real previewPos: 0
    readonly property var lines: previewMode ? previewLines : Lyrics.lines
    // The track has changed but the text is still from the previous one: dim it and do not process (otherwise position 0 starts
    // a scroll cascade over all lines of the old text — several heavy frames in a row).
    readonly property bool stale: !previewMode && Lyrics.trackId !== Player.trackId
    readonly property bool live: visible && !stale && lines.length > 0
    onLiveChanged: if (live && !previewMode) { jumpTimer.restart(); sync(Player.positionMs / 1000); tick() }

    // The font is rasterized at the full size of the active line from the start (fontSize * activeScale):
    // then the active line's layer scale is exactly 1.0 (100% native pixel sharpness, no smearing),
    // while inactive lines shrink smoothly via GPU Scale (idleScale / activeScale) without a layout rebuild!
    readonly property real activeFontPx: Math.round(fontSize * activeScale)
    readonly property real rowH: activeFontPx * 1.22
    readonly property real lineGap: fontSize * 0.56 * root.spacingScale
    readonly property real liftPx: fontSize * 0.075 * root.wordBounce
    readonly property real sidePad: Math.max(12, fontSize * 0.4)
    readonly property var scrollBezier: [0.22, 1.0, 0.36, 1.0, 1.0, 1.0]
    readonly property url fillShader: Qt.resolvedUrl("shaders/wordfill.frag.qsb")

    property real pos: 0
    property int activeIndex: -1
    // Forces a re-layout of the words: the preview sets its lines before the delegates exist,
    // and the layout binding cannot see their implicit widths until something wakes it.
    property int layoutRevision: 0
    property int _targetScrollIndex: -1
    readonly property int scrollIndex: _targetScrollIndex

    signal shifted(real dy)

    onLinesChanged: {
        jumpTimer.restart()      // new text takes its place without animation
        following = true
        manualAnim.running = false
        activeIndex = -1
        _targetScrollIndex = -1
        rebuild()
        tick()
    }

    // ── lines ───────────────────────────────────────────────────────────────
    // Text of 40–60 lines is hundreds of word items. Created all at once (and in two views as well:
    // the player and the side panel), they caused a ~300 ms stall on every track change. So lines
    // are built only in the visible view, a few per frame at a time.
    property var _lines: []                // the text the delegates were built for
    property var _pending: null            // new text waits until the old lines are torn down
    property int _built: 0
    property bool _tearing: false
    readonly property int buildChunk: 3

    // Old lines are torn down in batches too (all at once — ~50 ms). Delegates are bound to _lines, so
    // the new text is inserted only after the old lines are gone.
    function rebuild() {
        _pending = lines
        if (lineModel.count > 0)
            _tearing = true
        else
            _adopt()
        builder.running = true
    }
    function _adopt() {
        _lines = _pending ?? []
        _pending = null
        _built = 0
    }
    onVisibleChanged: if (visible && _built < _lines.length) builder.running = true

    // The track changed — old lines can be removed without waiting for the new text (it is already dimmed)
    onStaleChanged: if (stale) staleTeardown.restart()
    Timer {
        id: staleTeardown
        interval: 180
        onTriggered: {
            if (root.stale && lineModel.count > 0) {
                root._tearing = true
                builder.running = true
            }
        }
    }

    ListModel { id: lineModel }
    FrameAnimation {
        id: builder
        running: false
        onTriggered: {
            if (root._tearing) {
                const n = Math.min(lineModel.count, root.buildChunk * 2)
                if (n > 0)
                    lineModel.remove(lineModel.count - n, n)
                if (lineModel.count === 0) {
                    root._tearing = false
                    if (root._pending !== null) {
                        root._adopt()
                    } else if (!root.stale && root.lines.length > 0) {
                        root._lines = root.lines
                        root._built = 0
                    } else {
                        root._lines = []
                        root._built = 0
                    }
                }
                return
            }
            if (!root.visible || root._built >= root._lines.length) {
                running = false
                return
            }
            const until = Math.min(root._lines.length, root._built + root.buildChunk)
            for (; root._built < until; root._built++)
                lineModel.append({ line: root._built })
            jumpTimer.restart()               // while it builds — no scroll cascade
            if (root._built >= root._lines.length) {
                running = false
                // The words only exist now: wake the layout, otherwise they all sit at x=0.
                root.layoutRevision++
                root.tick()
            }
        }
    }

    // ── audio sync (PLL) ─────────────────────────────────────────────────────
    // The player sends the position several times a second; between updates the clock runs continuously with
    // playback rate taken into account. The error is mixed in by a low-pass filter (τ = 300 ms), as
    // in Spicy Lyrics, rather than by a speed multiplier on every update: with a rare step the correction
    // by speed overshot the target, and the text ran either early or late. A large jump (seek,
    // track change) sets the clock immediately. syncLeadSec (80 ms) — lead for perception and frames.
    property real _basePos: 0
    property real _baseTime: 0
    property real _posOffset: 0
    property real _measuredPos: 0
    property real _measuredAt: 0
    property bool _measured: false
    property real _lastTick: 0
    // Spicy Lyrics uses a 100 ms forward lead for the same reason; 80 ms leaves room
    // for the audio device buffer without pushing the sweep visibly ahead.
    property real syncLeadSec: 0.08
    readonly property real clockTau: 0.30
    property real _seekTarget: 0
    property real _seekStartMs: 0
    property real _seekGuardUntil: 0
    // while the DJ talks, the track stays put: the position does not advance even though the player is "playing".
    // The same while the track is still loading: "playing" is optimistic then, but there is no sound.
    readonly property bool advancing: Player.playing && !Player.narrating && !Player.buffering
    property bool _wasPlaying: advancing
    onAdvancingChanged: if (!previewMode) resyncPlayback()

    function rawPlayerPos() {
        if (!_wasPlaying)
            return _basePos
        const dt = Math.max(0, (Date.now() - _baseTime) / 1000)
        const spd = Settings.playbackSpeed > 0 ? Settings.playbackSpeed : 1.0
        return _basePos + dt * spd
    }
    function playerPos() {
        return Math.max(0, rawPlayerPos() + _posOffset + (_wasPlaying ? syncLeadSec : 0))
    }
    function sync(p) {
        _basePos = Math.max(0, p)
        _baseTime = Date.now()
        _posOffset = 0
        _measured = false
        _lastTick = 0
    }

    // Pause/buffering: we return to the measured position, not the frozen one
    function resyncPlayback() {
        if (root.advancing && !root._wasPlaying)
            root.sync(Player.positionMs / 1000)
        else
            root.sync(root.rawPlayerPos())
        root._wasPlaying = root.advancing
    }

    Connections {
        target: Player
        function onPositionChanged() {
            if (!root.live || root.previewMode)
                return
            const now = Date.now()
            const measured = Player.positionMs / 1000
            // After clicking a line the player may keep sending the old pre-seek position for up to ~1 s
            if (now < root._seekGuardUntil && Math.abs(measured - root._seekTarget) > 0.35) {
                if (now - root._seekStartMs < 1000)
                    root._seekGuardUntil = now + 180
                return
            }
            if (Math.abs(measured - (root.rawPlayerPos() + root._posOffset)) > 0.5) {
                // Seek, track change or a large gap: the clock is set immediately
                root.sync(measured)
            } else {
                root._measuredPos = measured
                root._measuredAt = now
                root._measured = true
            }
            root.tick()
        }
        function onStateChanged() { if (!root.previewMode) root.resyncPlayback() }
    }

    Connections {
        target: Settings
        function onLyricsSyncMsChanged() { root.tick() }
    }

    // The measured position error is mixed into the clock smoothly: between player updates
    // the position is extrapolated, not chased with a speed jump.
    function tickClock() {
        const now = Date.now()
        if (_measured && advancing) {
            const spd = Settings.playbackSpeed > 0 ? Settings.playbackSpeed : 1.0
            const measuredNow = _measuredPos + Math.max(0, (now - _measuredAt) / 1000) * spd
            const err = measuredNow - (rawPlayerPos() + _posOffset)
            const dt = _lastTick > 0 ? Math.min(0.1, Math.max(0, (now - _lastTick) / 1000)) : 0.016
            _posOffset += err * (1 - Math.exp(-dt / clockTau))
        }
        _lastTick = now
    }

    function tick() {
        if (root.previewMode) {
            pos = root.previewPos
        } else {
            tickClock()
            pos = playerPos() + Settings.lyricsSyncMs / 1000
        }
        const L = lines

        // Highlight and fill follow the synced position exactly: exactly one line is active at any time
        const at = pos
        let i = activeIndex
        if (i >= L.length || (i >= 0 && L[i].t > at) || i < -1) {
            let lo = 0, hi = L.length - 1
            i = -1
            while (lo <= hi) {
                const mid = (lo + hi) >> 1
                if (L[mid].t <= at) { i = mid; lo = mid + 1 } else hi = mid - 1
            }
        } else {
            while (i + 1 < L.length && L[i + 1].t <= at)
                i++
        }
        if (i !== activeIndex)
            activeIndex = i

        // Scroll lead and preparation of the incoming line: the column and scale start moving slightly early
        const atScroll = pos + scrollAheadMs / 1000
        let si = _targetScrollIndex
        if (si >= L.length || (si >= 0 && L[si].t > atScroll) || si < -1) {
            let lo = 0, hi = L.length - 1
            si = -1
            while (lo <= hi) {
                const mid = (lo + hi) >> 1
                if (L[mid].t <= atScroll) { si = mid; lo = mid + 1 } else hi = mid - 1
            }
        } else {
            while (si + 1 < L.length && L[si + 1].t <= atScroll)
                si++
        }
        if (si !== _targetScrollIndex)
            _targetScrollIndex = si
    }

    // The word wave is computed from the position every frame: on a shared 30 Hz clock it would step
    // on 120–240 Hz monitors. Software rendering (lowPower) — on the shared Ticker clock (~30 Hz).
    FrameAnimation {
        running: root.live && !root.previewMode && root.advancing && !Theme.lowPower
        onTriggered: root.tick()
    }
    TickAnimation {
        running: root.live && !root.previewMode && root.advancing && Theme.lowPower
        onTriggered: root.tick()
    }

    function seekTo(t) {
        // lines are shown with a sync offset — we seek to where the line actually sounds
        const at = Math.max(0, t + 0.02 - Settings.lyricsSyncMs / 1000)
        const now = Date.now()
        _seekTarget = at
        _seekStartMs = now
        _seekGuardUntil = now + 350
        Player.seekMs(Math.round(at * 1000))
        sync(at)
        tick()
    }

    function wordProgress(w, p) {
        if (p <= w.s) return 0
        if (p >= w.e) return 1
        if (!w.p) return (p - w.s) / Math.max(0.001, w.e - w.s)
        let total = 0, done = 0
        for (const part of w.p) total += part[0]
        for (const part of w.p) {
            if (p >= part[2]) done += part[0]
            else if (p > part[1]) done += part[0] * (p - part[1]) / Math.max(0.001, part[2] - part[1])
        }
        return Math.min(1, done / total)
    }

    function easeOut(x) {
        return 1 - Math.pow(1 - Math.max(0, Math.min(1, x)), 3)
    }

    TextMetrics {
        id: spaceMetrics
        font.family: Theme.fontFamily
        font.pixelSize: root.activeFontPx
        font.variableAxes: Theme.fontAxes(root.activeFontPx, 700)
        text: " "
    }

    // ── no lyrics ───────────────────────────────────────────────────────────
    EmptyState {
        anchors.centerIn: parent
        width: Math.min(parent.width - 32, 380)
        visible: !root.previewMode && !root.stale && root.lines.length === 0 && Lyrics.status !== "loading"
        icon: "title"
        shape: "flower6"
        title: !Player.hasTrack ? I18n.t("Nothing's playing")
             : Player.isEpisode ? I18n.t("Episodes don't have lyrics")
             : Lyrics.status === "error" ? I18n.t("Lyrics didn't load")
             : I18n.t("No lyrics")
        text: !Player.hasTrack ? I18n.t("Lyrics for the playing track will appear here")
            : Player.isEpisode ? I18n.t("Only songs have lyrics")
            : Lyrics.status === "error" ? I18n.t("Sources didn't respond — I'll try again on the next track")
            : I18n.t("No source has lyrics for this track")
    }
    LoadingIndicator {
        anchors.centerIn: parent
        visible: !root.previewMode && (Lyrics.status === "loading" || (root.stale && Player.hasTrack))
    }

    // ── text backdrop ───────────────────────────────────────────────────────
    Rectangle {
        id: scrimRect
        anchors.fill: parent
        anchors.topMargin: 8
        anchors.bottomMargin: 8
        anchors.leftMargin: Math.max(4, root.sidePad * 0.25)
        anchors.rightMargin: Math.max(4, root.sidePad * 0.25)
        radius: Theme.shape.extraLarge
        color: Theme.surface
        opacity: root.lines.length > 0 && !root.stale ? root.scrimAlpha : 0
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: root.motion ? 260 : 0 } }
    }

    // ── lyrics ──────────────────────────────────────────────────────────────
    Item {
        id: viewport
        anchors.fill: parent
        clip: true
        visible: root.lines.length > 0 && opacity > 0
        opacity: root.stale || root._tearing ? 0 : 1
        Behavior on opacity { NumberAnimation { duration: 160 } }

        // Edges dissolve into transparency (not into the background color — a cover can sit under the text). In software
        // rendering — no layer, edges are simply cut: a layer with changing text doubles frames (Qt asks
        // for the next one after every content change) and itself costs an extra pass over the whole text
        layer.enabled: !Theme.lowPower
        layer.effect: ShaderEffect {
            property real fadeTop: root.fadeTopPx / Math.max(1, viewport.height)
            property real fadeBottom: root.fadeBottomPx / Math.max(1, viewport.height)
            property real cut: 0
            fragmentShader: Qt.resolvedUrl("shaders/edgefade.frag.qsb")
        }

        Column {
            id: col
            x: root.sidePad
            width: parent.width - root.sidePad * 2
            spacing: 0

            Repeater {
                id: lineRep
                model: lineModel
                delegate: LyricLine {}
            }
        }

        WheelHandler {
            target: null
            enabled: !root.previewMode
            acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
            orientation: Qt.Vertical
            property real fineUntil: 0   // Windows trackpad: fractions of a notch — immediately, no easing (see SmoothScroll)
            onWheel: event => {
                // the trackpad is recognized by the gesture phase (see SmoothScroll): pixels 1:1, wheel — in eased steps
                if (event.phase !== Qt.NoScrollPhase) {
                    root.userScroll(event.pixelDelta.y, false)
                } else {
                    const a = Math.abs(event.angleDelta.y)
                    const now = Date.now()
                    if (a % 120 !== 0)
                        fineUntil = now + 400
                    const notches = event.angleDelta.y !== 0 ? event.angleDelta.y / 120 : event.pixelDelta.y / 15
                    root.userScroll(notches * Settings.wheelStep, now >= fineUntil)
                }
            }
        }
        DragHandler {
            id: dragger
            target: null
            enabled: !root.previewMode
            xAxis.enabled: false
            property real _last: 0
            onActiveChanged: _last = 0
            onTranslationChanged: {
                if (!active)
                    return
                root.userScroll(translation.y - _last, false)
                _last = translation.y
            }
        }
    }

    Button {
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.bottom: parent.bottom
        anchors.bottomMargin: 20
        z: 2
        size: "xs"
        style: "tonal"
        icon: "lyrics"
        text: I18n.t("Back to current line")
        visible: opacity > 0
        opacity: !root.previewMode && !root.following && root.lines.length > 0 && !root.stale ? 1 : 0
        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
        onClicked: root.resumeFollowing()
    }

    Label {
        visible: root.showSource && root.lines.length > 0 && Lyrics.source !== ""
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.margins: 4
        text: Lyrics.kind === "word" ? I18n.t("{0} · word by word", Lyrics.source) : I18n.t("{0} · line by line", Lyrics.source)
        type: "labelSmall"
        color: Theme.fgSurfaceVariant
        opacity: 0.6
    }

    // The top fade does not reach the target line (we use scrollIndex, same as colTargetY)
    readonly property real fadeTopPxTarget: {
        const want = viewport.height * fadeTopPct
        const idx = Math.max(0, scrollIndex)
        const it = lineRep.count > idx ? lineRep.itemAt(idx) : null
        if (!it || !following)
            return want
        return Math.max(0, Math.min(want, colTargetY + restY(idx) - lineGap * 0.25))
    }
    readonly property real fadeTopPx: fadeTopPxTarget
    readonly property real fadeBottomPx: viewport.height * fadeBottomPct

    // Scrolling: the column jumps straight to the target and each line gets a compensating shift,
    // which decays to 0 with its own delay. The target is the "settled" layout: an expanding
    // pause moves only the lines below it, not the whole text back and forth.
    function restY(idx) {
        const it = lineRep.itemAt(idx)
        if (!it)
            return 0
        let y = it.y
        for (let j = 0; j < idx; j++) {
            const g = lineRep.itemAt(j)
            if (g && g.isGap)
                y += g.restHeight - g.height
        }
        return y
    }

    readonly property real colTargetY: {
        const idx = Math.max(0, scrollIndex)
        const it = lineRep.count > idx ? lineRep.itemAt(idx) : null
        if (!it)
            return viewport.height * anchorPos
        return viewport.height * anchorPos - (restY(idx) + it.restHeight / 2)
    }

    property int _scrollIndex: -2
    onColTargetYChanged: Qt.callLater(applyScroll)

    function applyScroll() {
        if (!following)
            return
        const dy = col.y - colTargetY
        col.y = colTargetY
        _scrollIndex = scrollIndex
        if (!jumpTimer.running && Math.abs(dy) > 0.5)
            shifted(dy)
    }

    Timer { id: jumpTimer; interval: 250 }

    // ── manual scroll ───────────────────────────────────────────────────────
    // While the user scrolls, the column stays where it was dragged (col.y) and the active line
    // is only highlighted; the return uses the same line cascade as a normal line change.
    property bool following: true
    property real _manualTarget: 0
    readonly property real manualMaxY: viewport.height * anchorPos
    readonly property real manualMinY: viewport.height * anchorPos - col.height

    function _clampManual(y) { return Math.max(manualMinY, Math.min(manualMaxY, y)) }
    function userScroll(dy, smooth) {
        if (lines.length === 0)
            return
        if (following) {
            following = false
            _manualTarget = col.y
        }
        const base = smooth && manualAnim.running ? _manualTarget : col.y
        _manualTarget = _clampManual(base + dy)
        if (smooth) {
            manualAnim.running = true
        } else {
            manualAnim.running = false
            col.y = _manualTarget
        }
        resumeTimer.restart()
    }
    function resumeFollowing() {
        resumeTimer.stop()
        manualAnim.running = false
        if (following)
            return
        following = true
        const dy = col.y - colTargetY
        col.y = colTargetY
        _scrollIndex = scrollIndex
        if (Math.abs(dy) > 0.5)
            shifted(dy)
    }

    Timer {
        id: resumeTimer
        interval: root.resumeMs
        onTriggered: if (!dragger.active) root.resumeFollowing(); else restart()
    }

    // wheel easing — as in SmoothScroll: exponential, whole pixels
    FrameAnimation {
        id: manualAnim
        running: false
        onTriggered: {
            const d = root._manualTarget - col.y
            if (Math.abs(d) < 0.5) {
                col.y = root._manualTarget
                running = false
                return
            }
            const step = d * (1 - Math.exp(-frameTime * 14))
            col.y = Math.round(col.y + (Math.abs(step) < 1 ? Math.sign(d) : step))
        }
    }

    // ── line ────────────────────────────────────────────────────────────────
    component LyricLine: Item {
        id: line

        required property int index
        readonly property var modelData: root._lines[index] ?? ({ gap: true, t: 0, e: 0 })

        readonly property bool isGap: !!modelData.gap
        readonly property bool isActive: index === root.activeIndex
        // Exactly one line is ever active (isActive): two lines never sing at the same time.
        // The incoming line starts smoothly opening in scale and crispness in sync with the scroll lead
        // (isTarget), while the outgoing line keeps its shader and smoothly fades over 340 ms after the line change
        // (outgoingTailSec), so the transition between lines in Karaoke, Spicy and Classic is perfectly smooth.
        readonly property bool filling: !isGap && isActive
        readonly property real leadSec: Math.max(0.14, root.scrollAheadMs / 1000)
        readonly property bool isTarget: !isGap && (isActive || (root.following && index === root.activeIndex + 1 && ((modelData.t ?? 0) - root.pos) <= leadSec))
        readonly property real outgoingElapsed: (root.activeIndex >= 0 && index === root.activeIndex - 1)
            ? Math.max(0, root.pos - (root.lines[root.activeIndex]?.t ?? modelData.e ?? 0))
            : 999
        readonly property bool isOutgoing: !isGap && outgoingElapsed < 0.34
        readonly property bool wordsAnimating: !isGap && (isActive || isTarget || isOutgoing)
        readonly property int dist: root.activeIndex < 0 ? index + 1 : Math.abs(index - root.activeIndex)
        readonly property real gapProgress: isGap && isActive ? Math.max(0, Math.min(1, (root.pos - (modelData.t ?? 0)) / Math.max(0.1, (modelData.e ?? 0) - (modelData.t ?? 0)))) : 0
        readonly property bool hovered: hover.hovered

        // In Spicy, in Karaoke and with manual line scaling the font is always rendered
        // at the full activeFontPx size (scale 1.0 on the active line = 100% crispness, no smearing),
        // while inactive lines are softly shrunk to idleNormScale via GPU Scale without rebuilding the text!
        readonly property real idleNormScale: root.idleScale / Math.max(1.0, root.activeScale)
        readonly property real targetScale: !isGap ? (isTarget ? 1.0 : idleNormScale) : 1.0
        property real lineScale: targetScale
        Behavior on lineScale {
            enabled: root.motion && !Theme.lowPower
            NumberAnimation {
                duration: root.karaoke ? 380 : (root.spicy ? 320 : 300)
                easing.type: Easing.BezierSpline
                easing.bezierCurve: root.scrollBezier
            }
        }
        readonly property real lineSize: root.activeFontPx
        readonly property real rowH: root.rowH
        // Bumped as each word delegate appears: the layout needs their real widths.
        property int layoutRevision: 0

        // Line visibility filter (slider from 1 to 15 / All)
        readonly property bool inLineRange: {
            if (root.maxVisibleDist >= 999)
                return true
            return root.activeIndex < 0 ? index <= root.maxVisibleDist : dist <= root.maxVisibleDist
        }

        width: col.width
        // A collapsed pause keeps a hair of height: Column does not lay out zero-size items.
        // It expands along with the scroll (scrollIndex) so the column does not jitter.
        readonly property real restHeight: isGap
            ? ((isActive || index === root.scrollIndex) ? root.rowH * 1.1 + root.lineGap : 0.01)
            : wordsBox.height + root.lineGap
        height: restHeight
        clip: isGap

        Behavior on height {
            enabled: line.isGap
            SequentialAnimation {
                PauseAnimation { duration: Math.max(0, Math.min(7, line.index - Math.max(0, root.scrollIndex) + 1)) * root.scrollStagger }
                NumberAnimation { duration: root.scrollDuration; easing.type: Easing.BezierSpline; easing.bezierCurve: root.scrollBezier }
            }
        }

        // New text appearing: lines rise from below in a wave, starting from the active one.
        // Lines outside the range softly dissolve to 0, preserving the layout.
        property real appear: 0
        property real rangeOpacity: inLineRange ? 1 : 0
        Behavior on rangeOpacity {
            NumberAnimation { duration: root.motion ? 260 : 0; easing.type: Easing.OutCubic }
        }
        opacity: appear * rangeOpacity
        transform: [
            Translate { id: shift },
            Translate { y: (1 - line.appear) * root.fontSize * 1.1 }
        ]
        Component.onCompleted: Qt.callLater(() => {
            const rel = line.index - Math.max(0, root.activeIndex - 2)
            if (rel < 0 || rel > 14) {
                line.appear = 1
                return
            }
            appearAnim.delayMs = root.motion ? (root.spicy ? 60 + rel * 35 : 110 + rel * 50) : 0
            appearAnim.start()
        })
        SequentialAnimation {
            id: appearAnim
            property int delayMs: 0
            PauseAnimation { duration: appearAnim.delayMs }
            NumberAnimation {
                target: line
                property: "appear"
                to: 1
                duration: root.motion ? (root.spicy ? 480 : 620) : 0
                easing.type: Easing.BezierSpline
                easing.bezierCurve: Theme.motion.emphasizedDecelerate
            }
        }

        Connections {
            target: root
            function onShifted(dy) {
                const wasMoving = scrollAnim.running
                scrollAnim.stop()
                const anchor = root.scrollIndex >= 0 ? root.scrollIndex : Math.max(0, root.activeIndex)
                const rel = line.index - anchor
                // lines off screen do not need animation
                const sy = col.y + line.y + shift.y + dy
                if (sy > viewport.height * 2 || sy + line.height < -viewport.height) {
                    shift.y = 0
                    return
                }
                shift.y += dy
                scrollAnim.delayMs = wasMoving ? 0 : Math.max(0, Math.min(7, rel + 1)) * root.scrollStagger
                scrollAnim.interrupted = wasMoving
                scrollAnim.start()
            }
        }

        SequentialAnimation {
            id: scrollAnim
            property int delayMs: 0
            property bool interrupted: false
            PauseAnimation { duration: scrollAnim.delayMs }
            NumberAnimation {
                target: shift
                property: "y"
                to: 0
                duration: root.scrollDuration
                easing.type: scrollAnim.interrupted ? Easing.OutCubic : Easing.BezierSpline
                easing.bezierCurve: root.scrollBezier
            }
        }

        Rectangle {
            x: -root.sidePad * 0.6
            y: -root.lineGap * 0.3
            width: line.width + root.sidePad * 1.2
            height: wordsBox.height + root.lineGap * 0.6
            radius: Theme.shape.large
            color: root.textColor
            opacity: line.hovered && !line.isGap ? 0.08 : 0
            Behavior on opacity { NumberAnimation { duration: 160 } }
        }

        Item {
            id: wordsBox
            visible: !line.isGap
            width: line.width
            height: line.isGap ? 0 : Math.max(1, layout.rows) * line.rowH

            // Smooth vertical slide when a line enters/leaves and in "Whole line" mode
            property real wholeLineLift: {
                if (!root.motion)
                    return 0
                if (line.filling && root.fillMode === "line")
                    return -root.liftPx * (root.spicy ? 0.85 : 0.60)
                if (root.karaoke && !line.isTarget && !line.filling)
                    return (root.activeIndex >= 0 && line.index < root.activeIndex) ? -root.fontSize * 0.045 : root.fontSize * 0.045
                return 0
            }
            Behavior on wholeLineLift {
                enabled: root.motion && !Theme.lowPower
                NumberAnimation {
                    duration: root.karaoke ? 360 : 260
                    easing.type: Easing.BezierSpline
                    easing.bezierCurve: root.scrollBezier
                }
            }

            transform: Translate {
                y: wordsBox.wholeLineLift
            }

            // Smooth line opacity without jumps when the word shader is toggled:
            // wordsBox.opacity always follows the smooth fadeOpacity curve (0.35 ↔ 1.0),
            // and inside the shader the unsung parts of words are compensated via childDim = dimAlpha / fadeOpacity,
            // so unsung words always keep the exact brightness of root.dimAlpha without double dimming,
            // while the outgoing line softly fades from 1.0 to inactiveOpacity as the scale shrinks!
            readonly property real inactiveOpacity: root.fillMode === "off"
                ? (line.hovered ? 0.92 : 0.78)
                : Math.max(
                    0.12,
                    root.dimAlpha * (line.hovered ? 1.85 : (1 - (root.spicy ? 0.11 : 0.09) * Math.max(0, line.dist - 1)))
                )
            property real fadeOpacity: (line.filling || (line.isTarget && root.fillMode === "word")) ? 1.0 : inactiveOpacity
            Behavior on fadeOpacity {
                NumberAnimation {
                    duration: root.motion ? (root.karaoke ? 320 : (root.spicy ? 230 : 280)) : 0
                    easing.type: Easing.OutCubic
                }
            }
            readonly property real childDim: Math.min(1.0, root.dimAlpha / Math.max(0.15, fadeOpacity))
            opacity: fadeOpacity

            // Inactive lines are blurred statically without a blur animation,
            // so Qt draws the FBO once and caches the texture.
            // The blur is kept during scaling so the layer does not flicker when toggled.
            readonly property real lineBlur: (root.spicy ? 0.34 : 0.28) * root.blurAmount
            readonly property bool blurred: root.blurInactive && line.inLineRange && line.dist < 6
                && !line.isTarget && !line.filling && !line.hovered
            layer.enabled: blurred
            layer.smooth: true
            layer.effect: MultiEffect {
                blurEnabled: true
                blurMax: root.spicy ? 18 : 16
                blur: wordsBox.lineBlur
                autoPaddingEnabled: true
            }

            // Word wrapping with support for left, center and right alignment.
            // Since lineScale == 1.0 on the active line, the available width equals the full column width!
            readonly property var layout: {
                const revision = root.layoutRevision + line.layoutRevision
                const n = wordRep.count
                const avail = Math.max(1, width)
                const pos = []
                const widths = []
                let x = 0, row = 0
                for (let i = 0; i < n; i++) {
                    const it = wordRep.itemAt(i)
                    const w = it ? it.implicitWidth : 0
                    const tw = it ? it.textWidth : 0
                    if (x > 0 && x + tw > avail) {
                        row++
                        x = 0
                    }
                    pos.push({ x: x, row: row })
                    x += w
                    widths[row] = x - (it && it.spaced ? Math.max(0, w - tw) : 0)
                }
                let maxW = 0
                for (let r = 0; r <= row; r++)
                    if ((widths[r] ?? 0) > maxW) maxW = widths[r]
                if (root.align === "center") {
                    for (let i = 0; i < pos.length; i++)
                        pos[i].x += Math.max(0, (maxW - (widths[pos[i].row] ?? 0)) / 2)
                } else if (root.align === "right") {
                    for (let i = 0; i < pos.length; i++)
                        pos[i].x += Math.max(0, maxW - (widths[pos[i].row] ?? 0))
                }
                return { pos: pos, rows: row + 1, lineWidth: maxW }
            }

            Item {
                id: lineWords
                x: root.align === "center"
                    ? Math.max(0, (wordsBox.width - (wordsBox.layout?.lineWidth ?? 0)) / 2)
                    : (root.align === "right" ? Math.max(0, wordsBox.width - (wordsBox.layout?.lineWidth ?? 0)) : 0)
                width: Math.max(1, wordsBox.layout?.lineWidth ?? 0)
                height: wordsBox.height

                transform: Scale {
                    origin.x: root.align === "center" ? lineWords.width / 2 : (root.align === "right" ? lineWords.width : 0)
                    origin.y: lineWords.height / 2
                    xScale: line.lineScale
                    yScale: line.lineScale
                }

                Repeater {
                    id: wordRep
                    model: line.isGap ? [] : line.modelData.w
                    onItemAdded: line.layoutRevision++
                    delegate: LyricWord {
                        lineActive: line.filling
                        lineOutgoing: line.isOutgoing
                        outgoingElapsed: line.outgoingElapsed
                        wordAnimating: line.wordsAnimating
                        shaderDimAlpha: wordsBox.childDim
                        // 1 — the word is already sung, 2 — on the active line, 0 — not sung yet
                        sungState: (root.activeIndex >= 0 && line.index < root.activeIndex) ? 1
                                 : (line.filling ? 2 : 0)
                        fontSize: line.lineSize
                        rowH: line.rowH
                        useWordShader: line.wordsAnimating && root.fillMode === "word"
                        x: wordsBox.layout.pos[index]?.x ?? 0
                        y: (wordsBox.layout.pos[index]?.row ?? 0) * line.rowH
                    }
                }
            }
        }

        // Pause: three shapes "breathe" and fill
        Row {
            visible: line.isGap
            anchors.verticalCenter: parent.verticalCenter
            anchors.verticalCenterOffset: -root.lineGap / 2
            x: root.align === "center"
                ? Math.max(0, (parent.width - width) / 2)
                : (root.align === "right" ? Math.max(0, parent.width - width - root.fontSize * 0.1) : root.fontSize * 0.1)
            spacing: root.fontSize * 0.24
            transformOrigin: Item.Center
            scale: {
                if (!line.isActive)
                    return 0.6
                const breathe = 1 + 0.09 * Math.sin(root.pos * Math.PI * 2 / 1.9)
                const tail = Math.max(0, Math.min(1, ((line.modelData.e ?? 0) - root.pos) / 0.45))
                return breathe * (0.35 + 0.65 * root.easeOut(tail))
            }
            opacity: line.isActive ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: 300 } }

            // Instead of dots — three small MD3E shapes that "fill up" one after another and slowly rotate
            Repeater {
                model: ["cookie4", "clover4", "cookie6"]
                MorphShape {
                    required property int index
                    required property string modelData
                    readonly property real fill: Math.max(0, Math.min(1, line.gapProgress * 3 - index))
                    width: root.fontSize * 0.6
                    height: width
                    shape: fill >= 1 ? "cookie9" : modelData
                    color: root.textColor
                    opacity: root.dimAlpha + (1 - root.dimAlpha) * fill
                    rotation: line.isActive ? root.pos * (index % 2 ? -40 : 50) + index * 30 : 0
                }
            }
        }

        // Handlers, not MouseArea: a MouseArea would swallow the press and the text drag would never start
        HoverHandler {
            id: hover
            enabled: !root.previewMode && !line.isGap && line.inLineRange
            cursorShape: Qt.PointingHandCursor
        }
        TapHandler {
            enabled: !root.previewMode && !line.isGap && line.inLineRange
            onTapped: {
                root.seekTo(line.modelData.t)
                root.resumeFollowing()
            }
        }
    }

    // ── word ────────────────────────────────────────────────────────────────
    component LyricWord: Item {
        id: word

        required property var modelData
        required property int index
        property bool lineActive: false
        property bool lineOutgoing: false
        property real outgoingElapsed: 999
        property bool wordAnimating: false
        property bool useWordShader: false
        property real shaderDimAlpha: root.dimAlpha
        property real fontSize: root.activeFontPx
        property real rowH: root.rowH

        readonly property string txt: modelData.x.replace(/\s+$/, "")
        readonly property bool spaced: /\s$/.test(modelData.x)
        readonly property real textWidth: label.implicitWidth
        readonly property real dur: Math.max(0.001, (modelData.e ?? 0) - (modelData.s ?? 0))
        readonly property bool held: dur >= 0.95 && txt.length <= 14

        // Spicy: a long word is drawn letter by letter, each rising, growing and glowing with a time offset.
        // Only simple scripts without combining marks: for RTL, combining accents and emoji
        // per-letter layout breaks shaping and width — there a plain label remains.
        // It does not change mid-word, so the layout does not jump.
        readonly property bool lettered: root.spicy && root.motion && !Theme.lowPower
            && root.fillMode === "word" && held && txt.length >= 3
            && /^[A-Za-z\u00C0-\u024F\u0400-\u04FF'\u2019-]+$/.test(txt)
        readonly property var letters: lettered ? Array.from(txt) : []
        // Strength grows with duration (square root after 2 s, cube before), as in Spicy Lyrics
        readonly property real waveAmount: {
            const x = Math.max(1.0, dur) / 2.0
            return Math.min(1.2, (x > 1 ? Math.sqrt(x) : x * x * x) * 0.6)
        }
        // Letter i impulse, 0..1..0: it starts within the first 40% of the word and outlives the word by at most 0.3 s
        function letterPulse(i, n) {
            const start = (modelData.s ?? 0) + (n > 1 ? i / (n - 1) : 0) * dur * 0.4
            const end = Math.min(start + Math.max(1.0, dur), (modelData.e ?? 0) + 0.3)
            if (end <= start)
                return 0
            const x = (root.pos - start) / (end - start)
            return (x <= 0 || x >= 1) ? 0 : Math.sin(Math.PI * x)
        }
        // On the active line and during the 340 ms smooth exit of the previous line (lineOutgoing)
        // the fill reaches the end with no instant jump at the line change.
        readonly property real progress: {
            if (lineActive)
                return root.wordProgress(modelData, root.pos)
            if (lineOutgoing)
                return Math.max(root.wordProgress(modelData, root.pos), Math.min(1, outgoingElapsed / 0.18))
            return sungState === 1 ? 1 : 0
        }
        required property int sungState

        // A smooth Hermite curve (smoothstep) and a three-phase envelope (Attack → Sustain → Release)
        // in real time (seconds), like the springs in Spicy Lyrics:
        // the word quickly rises over ~200 ms, rides the wave crest while the note sounds, and softly
        // settles over ~220 ms afterwards, forming a continuous running wave across neighboring words
        // in all three styles (Spicy, Karaoke and Classic).
        function ease(x) { return x <= 0 ? 0 : (x >= 1 ? 1 : x * x * (3 - 2 * x)) }

        readonly property real envelope: {
            if (!wordAnimating || root.fillMode !== "word")
                return 0
            const s = (modelData.s ?? 0) - 0.015
            const e = (modelData.e ?? 0)
            const t = root.pos
            const attack = Math.min(0.22, Math.max(0.13, dur * 0.62))
            const fallStart = Math.max(e, s + attack)
            const release = root.karaoke ? 0.24 : 0.21
            if (t <= s)
                return 0
            if (t >= fallStart + release)
                return 0
            if (t < s + attack)
                return word.ease((t - s) / attack)
            if (t <= fallStart)
                return 1.0
            return 1.0 - word.ease((t - fallStart) / release)
        }

        readonly property bool alreadySung: sungState === 1 || ((lineActive || lineOutgoing) && root.pos >= (modelData.e ?? 0))

        // Word scale: in Spicy (0.96 → 1.0 + 0.072*bounce → 1.00), in Karaoke — a soft impulse (1.0 → 1.032 → 1.0)
        readonly property real sungScale: {
            if (root.fillMode !== "word")
                return 1.0
            const env = word.envelope
            if (root.spicy) {
                const rest = word.alreadySung ? 1.0 : 0.96
                if (env <= 0.001)
                    return rest
                const peak = 1.0 + 0.072 * root.wordBounce
                const vibrato = (word.held && env > 0.85) ? 0.014 * root.wordBounce * Math.sin((root.pos - (modelData.s ?? 0)) * Math.PI * 4.5) : 0
                return rest + (peak - rest) * env + vibrato
            }
            if (root.karaoke) {
                return 1.0 + 0.032 * root.wordBounce * env
            }
            return 1.0 + 0.042 * word.glow * root.wordBounce
        }

        // Vertical word lift (the "wave"):
        // it rises smoothly along the envelope while the word sounds and softly returns to 0
        // with no sharp reset when switching to the next line.
        readonly property real sungLiftEm: {
            if (root.fillMode !== "word")
                return 0
            const env = word.envelope
            if (root.spicy) {
                const rest = word.alreadySung ? 0.0 : 0.014 * root.wordBounce
                if (env <= 0.001)
                    return rest
                const peak = -0.088 * root.wordBounce
                return rest + (peak - rest) * env
            }
            if (root.karaoke)
                return -0.055 * root.wordBounce * env
            return -0.048 * root.wordBounce * env
        }

        readonly property real glow: {
            if (!wordAnimating || root.fillMode !== "word")
                return 0
            if (root.spicy)
                return 0.90 * word.envelope * root.highlightMultiplier
            if (root.karaoke)
                return 0.25 * word.envelope * root.highlightMultiplier
            return (held ? Math.sin(Math.PI * progress) : 0) * root.highlightMultiplier
        }

        // The trailing space of a word comes from the overall metrics, scaled to the line size
        implicitWidth: label.implicitWidth + (spaced ? spaceMetrics.advanceWidth * (fontSize / Math.max(1, root.activeFontPx)) : 0)
        implicitHeight: rowH

        // The shader layer is enabled ONLY while the fill passes over the word or while the glow has not faded.
        // Before and after that the word is drawn as native text, with no FBO and no shader.
        readonly property bool sweeping: word.useWordShader
            && ((progress > 0 && progress < 1) || word.glow > 0.005)

        Text {
            id: label
            // Google Sans draws ≈1.36em tall while the line holds 1.22em: the layer texture without headroom
            // cut off the bottom descenders. We add height with padding — the text stays in place.
            readonly property real layerOverscan: Math.ceil(word.fontSize * 0.25)
            height: word.rowH + 2 * layerOverscan
            topPadding: layerOverscan
            bottomPadding: layerOverscan
            anchors.verticalCenter: parent.verticalCenter
            verticalAlignment: Text.AlignVCenter
            text: word.txt
            font.family: Theme.fontFamily
            font.pixelSize: word.fontSize
            font.weight: Theme.variableFont ? Font.Normal : Font.Bold
            font.variableAxes: Theme.fontAxes(word.fontSize, 700)
            color: root.textColor
            // An unsung word on the active/outgoing line is dimmed natively; a sung one is fully opaque
            opacity: word.useWordShader ? (word.sweeping ? 1.0 : (word.progress >= 1 ? 1.0 : word.shaderDimAlpha)) : 1.0
            // invisible, but still measuring: the word width must not wait for the letter Repeater
            visible: !word.lettered
            transformOrigin: Item.Center
            scale: word.sungScale

            transform: Translate {
                id: wordTranslate
                y: word.sungLiftEm * word.fontSize
                Behavior on y {
                    // Behavior works only outside the word animation (during the wave sungLiftEm changes continuously)
                    enabled: !word.wordAnimating && root.motion && !Theme.lowPower
                    NumberAnimation { duration: 105; easing.type: Easing.OutCubic }
                }
            }

            layer.enabled: word.sweeping && !word.lettered
            layer.smooth: true
            layer.effect: ShaderEffect {
                readonly property real featherPx: word.fontSize * (root.spicy ? 0.72 : (root.karaoke ? 0.14 : 0.45))
                readonly property real baseFeather: root.karaoke
                    ? Math.max(0.025, Math.min(0.10, featherPx / Math.max(1, label.width)))
                    : (root.spicy
                        ? Math.max(0.34, Math.min(0.62, featherPx / Math.max(1, label.width)))
                        : Math.min(0.5, featherPx / Math.max(1, label.width) / 2))
                property real feather: Math.max(0.005, baseFeather * root.highlightMultiplier)
                property real progress: -feather + word.progress * (1 + 2 * feather)
                property real dimAlpha: word.shaderDimAlpha
                property real glow: word.glow
                fragmentShader: root.fillShader
            }
        }
        // Kerning gives the prefix advanceWidth, not the sum of individual letter widths
        FontMetrics {
            id: letterMetrics
            font: label.font
        }

        Item {
            id: lettersBox
            visible: word.lettered
            width: label.implicitWidth
            height: word.rowH
            anchors.verticalCenter: parent.verticalCenter
            transformOrigin: Item.Center
            scale: word.sungScale
            transform: Translate {
                y: word.sungLiftEm * word.fontSize
                Behavior on y {
                    enabled: !word.wordAnimating && root.motion && !Theme.lowPower
                    NumberAnimation { duration: 105; easing.type: Easing.OutCubic }
                }
            }

            Repeater {
                model: word.letters

                Text {
                    id: letter
                    required property string modelData
                    required property int index
                    readonly property real pulse: word.lettered && word.wordAnimating ? word.letterPulse(index, word.letters.length) : 0
                    readonly property real centered: index - (word.letters.length - 1) / 2
                    readonly property real overscan: Math.ceil(word.fontSize * 0.25)
                    readonly property real fullWidth: Math.max(1, label.implicitWidth)
                    readonly property real a: x / fullWidth
                    readonly property real b: (x + implicitWidth) / fullWidth
                    readonly property bool sweepingLetter: word.useWordShader
                        && ((word.progress > 0 && word.progress < 1) || pulse > 0.005)

                    x: letterMetrics.advanceWidth(word.txt.slice(0, index + 1)) - letterMetrics.advanceWidth(modelData)
                    y: -overscan
                    height: word.rowH + 2 * overscan
                    topPadding: overscan
                    bottomPadding: overscan
                    verticalAlignment: Text.AlignVCenter
                    text: modelData
                    font.family: Theme.fontFamily
                    font.pixelSize: word.fontSize
                    font.weight: Theme.variableFont ? Font.Normal : Font.Bold
                    font.variableAxes: Theme.fontAxes(word.fontSize, 700)
                    color: root.textColor
                    opacity: word.useWordShader ? (sweepingLetter ? 1.0 : (word.progress >= 1 ? 1.0 : word.shaderDimAlpha)) : 1.0
                    transformOrigin: Item.Center
                    scale: 1.0 + 0.07 * word.waveAmount * root.wordBounce * pulse

                    transform: Translate {
                        // Spread no more than ~0.07 em at the word edge, so it does not eat the space before the next word
                        x: 0.07 * Math.min(1, word.waveAmount) * word.fontSize * (letter.centered / Math.max(1, word.letters.length / 2)) * letter.pulse
                        y: -0.025 * word.waveAmount * word.fontSize * root.wordBounce * letter.pulse
                    }

                    layer.enabled: sweepingLetter
                    layer.smooth: true
                    layer.effect: ShaderEffect {
                        // The same bound as for the whole word, recomputed into the letter's local coordinates
                        readonly property real featherWord: Math.max(0.005, Math.max(0.34, Math.min(0.62, word.fontSize * 0.72 / letter.fullWidth)) * root.highlightMultiplier)
                        readonly property real edge: -featherWord + word.progress * (1 + 2 * featherWord)
                        readonly property real span: Math.max(0.001, letter.b - letter.a)
                        property real progress: (edge - letter.a) / span
                        property real feather: featherWord / span
                        property real dimAlpha: word.shaderDimAlpha
                        property real glow: Math.max(word.glow, letter.pulse * 0.9 * word.waveAmount * root.highlightMultiplier)
                        fragmentShader: root.fillShader
                    }
                }
            }
        }
    }
}
