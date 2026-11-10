pragma Singleton
import QtQuick
import Materialfy.Core
import "ColorMix.js" as Mix

// MD3 Expressive tokens. Colors come from ThemeEngine and animate smoothly when the cover changes.
Item {
    id: root

    readonly property var engine: ThemeEngine
    readonly property bool dark: ThemeEngine.dark
    property int colorDuration: 800

    // ---- Color roles ---------------------------------------------------
    // A scheme change is ONE progress animation _p; all roles blend along it in sync (in OKLab).
    // The spec's onX roles are named fgX (onPrimary → fgPrimary, inverseOnSurface → fgInverseSurface):
    // QML treats names of the form onSomething as signal handlers, and a Behavior on them crashes the engine.

    // true while a scheme change is running: components disable their color Behaviors so there is no double animation.
    readonly property bool transitioning: _anim.running || (wipe !== null && wipe.running)

    // If a ThemeWipe is set — the scheme changes as a "curtain" toward wipeDirection (+1 right, -1 left),
    // otherwise — by a smooth animation of all colors.
    property Item wipe: null
    // Curtains for windows layered over the app (the island, the taskbar player) run together with the main one. Snapshots
    // with the old colors are taken in all windows, the scheme changes when all are ready, and the snapshots leave at once
    property var floatingWipes: []
    function addWipe(w) { floatingWipes = floatingWipes.filter(x => x !== w).concat([w]) }
    function removeWipe(w) { floatingWipes = floatingWipes.filter(x => x !== w) }
    // Window not focused: decorative infinite animations stop. Each such animation forces
    // a full window repaint at monitor refresh — the main background cost.
    property bool calm: false
    // Low-power PC mode: the setting "on" or (by default, "auto") — frames are drawn by the CPU, software rendering
    // (llvmpipe, WARP: no GPU driver; Main.qml reads it from the window's softwareRendering). Each frame there costs
    // tens of milliseconds of core time: decorative motion always stops, and the window gets no rounded-corner layer.
    property bool lowPower: false
    // Decorative infinite animations (shape and background drift, cover rotation and "breathing", the running wave) stop.
    // Loading indicators only look at calm: under software rendering they spin — that is not decoration.
    readonly property bool quiet: calm || lowPower
    // Minimal animations: the "Animations" setting or the system flag. Spatial durations
    // are zeroed, only short fades remain; waves and morphs are disabled by the components themselves.
    // Manually enabled low-power mode too: on a very weak machine transitions also stutter.
    readonly property bool reducedMotion: Settings.animations === "reduced" || Settings.lowPowerMode === "on"
        || (Settings.animations === "system" && Settings.systemReducedMotion === true)
    // The scheme color when nothing is playing
    readonly property color defaultSeed: "#8C6DFF"   // Ultraviolet, BRAND_SEED in bridges/theme.rs
    // The new scheme has been applied: instant — immediately under the curtain, otherwise a smooth animation has started.
    // What depends on the cover (the atmospheric background) changes in sync with the colors.
    signal schemeApplied(bool instant)
    property int wipeDirection: 1

    // public name → key in ThemeEngine.colors
    readonly property var _roles: ({
        background: "background",
        fgBackground: "onBackground",
        surface: "surface",
        surfaceDim: "surfaceDim",
        surfaceBright: "surfaceBright",
        surfaceContainerLowest: "surfaceContainerLowest",
        surfaceContainerLow: "surfaceContainerLow",
        surfaceContainer: "surfaceContainer",
        surfaceContainerHigh: "surfaceContainerHigh",
        surfaceContainerHighest: "surfaceContainerHighest",
        fgSurface: "onSurface",
        surfaceVariant: "surfaceVariant",
        fgSurfaceVariant: "onSurfaceVariant",
        outline: "outline",
        outlineVariant: "outlineVariant",
        inverseSurface: "inverseSurface",
        fgInverseSurface: "inverseOnSurface",
        shadow: "shadow",
        scrim: "scrim",
        surfaceTint: "surfaceTint",
        primary: "primary",
        primaryDim: "primaryDim",
        fgPrimary: "onPrimary",
        primaryContainer: "primaryContainer",
        fgPrimaryContainer: "onPrimaryContainer",
        inversePrimary: "inversePrimary",
        primaryFixed: "primaryFixed",
        primaryFixedDim: "primaryFixedDim",
        fgPrimaryFixed: "onPrimaryFixed",
        fgPrimaryFixedVariant: "onPrimaryFixedVariant",
        secondary: "secondary",
        secondaryDim: "secondaryDim",
        fgSecondary: "onSecondary",
        secondaryContainer: "secondaryContainer",
        fgSecondaryContainer: "onSecondaryContainer",
        secondaryFixed: "secondaryFixed",
        secondaryFixedDim: "secondaryFixedDim",
        fgSecondaryFixed: "onSecondaryFixed",
        fgSecondaryFixedVariant: "onSecondaryFixedVariant",
        tertiary: "tertiary",
        tertiaryDim: "tertiaryDim",
        fgTertiary: "onTertiary",
        tertiaryContainer: "tertiaryContainer",
        fgTertiaryContainer: "onTertiaryContainer",
        tertiaryFixed: "tertiaryFixed",
        tertiaryFixedDim: "tertiaryFixedDim",
        fgTertiaryFixed: "onTertiaryFixed",
        fgTertiaryFixedVariant: "onTertiaryFixedVariant",
        error: "error",
        errorDim: "errorDim",
        fgError: "onError",
        errorContainer: "errorContainer",
        fgErrorContainer: "onErrorContainer",
        primaryPaletteKeyColor: "primaryPaletteKeyColor",
        secondaryPaletteKeyColor: "secondaryPaletteKeyColor",
        tertiaryPaletteKeyColor: "tertiaryPaletteKeyColor",
        neutralPaletteKeyColor: "neutralPaletteKeyColor",
        neutralVariantPaletteKeyColor: "neutralVariantPaletteKeyColor",
        errorPaletteKeyColor: "errorPaletteKeyColor"
    })

    property var _fromLab: ({})
    property var _toLab: ({})
    property real _p: 1

    function _mix(name) {
        const key = _roles[name]
        const b = _toLab[key]
        if (!b)  // before Component.onCompleted
            return ThemeEngine.colors[key]
        return Mix.mix(_fromLab[key] ?? b, b, _p)
    }

    function _startTransition() {
        const from = {}
        for (const name in _roles)
            from[_roles[name]] = Mix.toLab(root[name])  // the currently visible color, even mid-transition
        _fromLab = from
        _toLab = Mix.labMap(ThemeEngine.colors)
        _p = 0
        _anim.restart()
        schemeApplied(false)
    }

    Component.onCompleted: {
        const lab = Mix.labMap(ThemeEngine.colors)
        _fromLab = lab
        _toLab = lab
    }

    function _applyNow() {
        _anim.stop()
        const lab = Mix.labMap(ThemeEngine.colors)
        _toLab = lab
        _fromLab = lab
        _p = 1
        schemeApplied(true)
    }

    Connections {
        target: ThemeEngine
        function onSchemeChanged() {
            // software rendering: immediately, without a curtain over the whole window (the CPU draws each of its frames in full)
            if (root.lowPower)
                root._applyNow()
            else if (root.wipe !== null && root.wipe.visible)
                root._wipeAll()
            else
                root._startTransition()
        }
    }

    property var _wiping: []
    property int _wipeLeft: 0
    function _wipeAll() {
        const ws = [wipe].concat(floatingWipes.filter(w => w && w.onScreen))
        _wiping = ws
        _wipeLeft = ws.length
        for (const w of ws)
            w.start(wipeDirection, root._wipeReady)
        _wipeTimeout.restart()
    }
    function _wipeReady(w) {
        if (_wipeLeft <= 0) {       // too late: the scheme has already changed
            w.reveal()
            return
        }
        if (--_wipeLeft === 0)
            _wipeGo()
    }
    function _wipeGo() {
        _wipeTimeout.stop()
        _wipeLeft = 0
        _applyNow()
        for (const w of _wiping)
            w.reveal()
        _wiping = []
    }
    // a window that is not drawn (hidden) gives no snapshot: no long wait
    Timer { id: _wipeTimeout; interval: 300; onTriggered: if (root._wipeLeft > 0) root._wipeGo() }

    NumberAnimation {
        id: _anim
        target: root
        property: "_p"
        from: 0
        to: 1
        duration: root.colorDuration
        easing.type: Easing.InOutCubic
    }

    readonly property color background: _mix("background")
    readonly property color fgBackground: _mix("fgBackground")
    readonly property color surface: _mix("surface")
    readonly property color surfaceDim: _mix("surfaceDim")
    readonly property color surfaceBright: _mix("surfaceBright")
    readonly property color surfaceContainerLowest: _mix("surfaceContainerLowest")
    readonly property color surfaceContainerLow: _mix("surfaceContainerLow")
    readonly property color surfaceContainer: _mix("surfaceContainer")
    readonly property color surfaceContainerHigh: _mix("surfaceContainerHigh")
    readonly property color surfaceContainerHighest: _mix("surfaceContainerHighest")
    readonly property color fgSurface: _mix("fgSurface")
    readonly property color surfaceVariant: _mix("surfaceVariant")
    readonly property color fgSurfaceVariant: _mix("fgSurfaceVariant")
    readonly property color outline: _mix("outline")
    readonly property color outlineVariant: _mix("outlineVariant")
    readonly property color inverseSurface: _mix("inverseSurface")
    readonly property color fgInverseSurface: _mix("fgInverseSurface")
    readonly property color shadow: _mix("shadow")
    readonly property color scrim: _mix("scrim")
    readonly property color surfaceTint: _mix("surfaceTint")
    readonly property color primary: _mix("primary")
    readonly property color primaryDim: _mix("primaryDim")
    readonly property color fgPrimary: _mix("fgPrimary")
    readonly property color primaryContainer: _mix("primaryContainer")
    readonly property color fgPrimaryContainer: _mix("fgPrimaryContainer")
    readonly property color inversePrimary: _mix("inversePrimary")
    readonly property color primaryFixed: _mix("primaryFixed")
    readonly property color primaryFixedDim: _mix("primaryFixedDim")
    readonly property color fgPrimaryFixed: _mix("fgPrimaryFixed")
    readonly property color fgPrimaryFixedVariant: _mix("fgPrimaryFixedVariant")
    readonly property color secondary: _mix("secondary")
    readonly property color secondaryDim: _mix("secondaryDim")
    readonly property color fgSecondary: _mix("fgSecondary")
    readonly property color secondaryContainer: _mix("secondaryContainer")
    readonly property color fgSecondaryContainer: _mix("fgSecondaryContainer")
    readonly property color secondaryFixed: _mix("secondaryFixed")
    readonly property color secondaryFixedDim: _mix("secondaryFixedDim")
    readonly property color fgSecondaryFixed: _mix("fgSecondaryFixed")
    readonly property color fgSecondaryFixedVariant: _mix("fgSecondaryFixedVariant")
    readonly property color tertiary: _mix("tertiary")
    readonly property color tertiaryDim: _mix("tertiaryDim")
    readonly property color fgTertiary: _mix("fgTertiary")
    readonly property color tertiaryContainer: _mix("tertiaryContainer")
    readonly property color fgTertiaryContainer: _mix("fgTertiaryContainer")
    readonly property color tertiaryFixed: _mix("tertiaryFixed")
    readonly property color tertiaryFixedDim: _mix("tertiaryFixedDim")
    readonly property color fgTertiaryFixed: _mix("fgTertiaryFixed")
    readonly property color fgTertiaryFixedVariant: _mix("fgTertiaryFixedVariant")
    readonly property color error: _mix("error")
    readonly property color errorDim: _mix("errorDim")
    readonly property color fgError: _mix("fgError")
    readonly property color errorContainer: _mix("errorContainer")
    readonly property color fgErrorContainer: _mix("fgErrorContainer")
    readonly property color primaryPaletteKeyColor: _mix("primaryPaletteKeyColor")
    readonly property color secondaryPaletteKeyColor: _mix("secondaryPaletteKeyColor")
    readonly property color tertiaryPaletteKeyColor: _mix("tertiaryPaletteKeyColor")
    readonly property color neutralPaletteKeyColor: _mix("neutralPaletteKeyColor")
    readonly property color neutralVariantPaletteKeyColor: _mix("neutralVariantPaletteKeyColor")
    readonly property color errorPaletteKeyColor: _mix("errorPaletteKeyColor")

    // State layer opacity
    readonly property QtObject stateLayer: QtObject {
        readonly property real hover: 0.08
        readonly property real focus: 0.10
        readonly property real pressed: 0.10
        readonly property real dragged: 0.16
        readonly property real disabledContent: 0.38
        readonly property real disabledContainer: 0.12
    }

    // ---- Typography ---------------------------------------------------------
    // Google Sans (OFL, embedded in assets/fonts). Google Sans Flex does not work: it has no Cyrillic.
    readonly property string fontFamily: _pickFamily(["Google Sans", "Roboto Flex", "Roboto", "Noto Sans"])
    readonly property string iconFamily: "Material Symbols Rounded"
    readonly property bool variableFont: fontFamily === "Google Sans"

    // Variable Google Sans axes: wght 400..700, opsz 17..18 (17 is the text cut for small sizes).
    function fontAxes(size, weight) {
        if (!variableFont)
            return {}
        return { "wght": Math.max(400, Math.min(700, weight)), "opsz": size < 16 ? 17 : 18 }
    }

    function _pickFamily(candidates) {
        const available = Qt.fontFamilies()
        for (const f of candidates)
            if (available.indexOf(f) >= 0)
                return f
        return Qt.application.font.family
    }

    // size / lineHeight / weight / letterSpacing (px)
    readonly property var type: ({
        displayLarge:   { size: 57, line: 64, weight: 400, tracking: -0.25 },
        displayMedium:  { size: 45, line: 52, weight: 400, tracking: 0 },
        displaySmall:   { size: 36, line: 44, weight: 400, tracking: 0 },
        headlineLarge:  { size: 32, line: 40, weight: 400, tracking: 0 },
        headlineMedium: { size: 28, line: 36, weight: 400, tracking: 0 },
        headlineSmall:  { size: 24, line: 32, weight: 400, tracking: 0 },
        titleLarge:     { size: 22, line: 28, weight: 400, tracking: 0 },
        titleMedium:    { size: 16, line: 24, weight: 500, tracking: 0.15 },
        titleSmall:     { size: 14, line: 20, weight: 500, tracking: 0.1 },
        bodyLarge:      { size: 16, line: 24, weight: 400, tracking: 0.5 },
        bodyMedium:     { size: 14, line: 20, weight: 400, tracking: 0.25 },
        bodySmall:      { size: 12, line: 16, weight: 400, tracking: 0.4 },
        labelLarge:     { size: 14, line: 20, weight: 500, tracking: 0.1 },
        labelMedium:    { size: 12, line: 16, weight: 500, tracking: 0.5 },
        labelSmall:     { size: 11, line: 16, weight: 500, tracking: 0.5 }
    })

    // ---- Shapes (corner radii) -----------------------------------------
    readonly property QtObject shape: QtObject {
        readonly property real none: 0
        readonly property real extraSmall: 4
        readonly property real small: 8
        readonly property real medium: 12
        readonly property real large: 16
        readonly property real largeIncreased: 20
        readonly property real extraLarge: 28
        readonly property real extraLargeIncreased: 32
        readonly property real extraExtraLarge: 48
        readonly property real full: 9999
    }

    // ---- Spacing -------------------------------------------------------------
    readonly property QtObject space: QtObject {
        readonly property real xs: 4
        readonly property real s: 8
        readonly property real m: 12
        readonly property real l: 16
        readonly property real xl: 24
        readonly property real xxl: 32
    }

    // ---- Motion --------------------------------------------------------------
    // MD3 Expressive springs are approximated with overshoot curves (OutBack),
    // effects (color, opacity) — without overshoot.
    readonly property QtObject motion: QtObject {
        readonly property int spatialFast: root.reducedMotion ? 0 : 350
        readonly property int spatialDefault: root.reducedMotion ? 0 : 500
        readonly property int spatialSlow: root.reducedMotion ? 0 : 650
        readonly property int effectsFast: 150
        readonly property int effectsDefault: 200
        readonly property int effectsSlow: 300
        readonly property real overshoot: 1.25

        readonly property var emphasized: [0.2, 0.0, 0.0, 1.0, 1.0, 1.0]
        readonly property var emphasizedDecelerate: [0.05, 0.7, 0.1, 1.0, 1.0, 1.0]
        readonly property var emphasizedAccelerate: [0.3, 0.0, 0.8, 0.15, 1.0, 1.0]
        readonly property var standard: [0.2, 0.0, 0.0, 1.0, 1.0, 1.0]
    }
}
