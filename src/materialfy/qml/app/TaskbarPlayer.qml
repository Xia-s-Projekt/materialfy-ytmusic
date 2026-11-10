import QtQuick
import QtQuick.Effects
import Md3
import Materialfy.Core

// Player in the Windows taskbar: an MD3 Expressive pill (black or in the cover's tone) — cover, title,
// artist and a thin progress wave, "play" and "next" ("back" — on hover). The window is embedded
// into Shell_TrayWnd (WindowShell.taskbarPlace, cpp/floating.cpp) or, if that fails (Windhawk, a second monitor),
// it sits above the panel it belongs to. Every 2 s it takes its place again (the panel changed size,
// icons shifted); Explorer carried the window away on restart — lost(), Main.qml recreates it.
// Press and drag the cover or the title — the widget slides along the panel and snaps to places (taskbarWidgetPos).
// Wheel — volume (the level shows on the cover), arrow keys and space on hover — like the island.
Window {
    id: root

    property var mainWindow: null
    property var targetScreen: null
    property int screenIndex: 0
    property bool embed: true
    signal lost()
    // --set floatingShot=<path.png>: a window shot after 9 s without embedding; --set taskbarTheme=dark|light — panel theme
    property string shotPath: ""
    property string forcedTheme: ""
    Timer {
        interval: 9000
        running: root.shotPath !== ""
        onTriggered: root.contentItem.grabToImage(result => result.saveToFile(root.shotPath))
    }

    screen: targetScreen || (mainWindow ? mainWindow.screen : null)

    readonly property int slotWidth: 264
    width: slotWidth
    height: 40
    color: "transparent"
    title: "Materialfy"
    flags: Qt.Tool | Qt.FramelessWindowHint | Qt.WindowStaysOnTopHint | Qt.WindowDoesNotAcceptFocus
    // Without a Qt owner: a window owned by the main one is hidden by Windows together with it (the owner is the panel, see C++)
    transientParent: null

    readonly property bool motion: !Theme.reducedMotion && !Theme.lowPower
    // the volume level is needed only for the cover "pulse"
    readonly property bool wantsLevels: visible && motion && Settings.coverPulse && Player.playing && Player.isLocal === true
    readonly property bool colorMode: Settings.floatingPlayerStyle === "color"
    readonly property bool _onCover: colorMode && Player.cover !== ""
    // On dark glass over the cover and on black — light ones; colored without a cover — a tonal container
    readonly property color ink: colorMode && !_onCover ? Theme.fgPrimaryContainer : "#ffffff"
    readonly property color inkDim: Qt.alpha(ink, 0.72)
    // on black — a "fixed" MD3 accent: equally light in light and dark themes
    readonly property color accent: colorMode ? ink : Theme.primaryFixedDim

    // 1 — in the taskbar, 2 — above it, 0 — nowhere (vertical, narrow or hidden panel): the window is hidden
    property int placement: 0
    property bool _ready: false
    property bool _fullscreen: false
    property bool dragActive: false
    visible: _ready && placement > 0 && !(placement === 2 && _fullscreen)
    // Qt resets the window's owner when showing it — restore the panel immediately, not after 2 s
    onVisibleChanged: if (visible) Qt.callLater(place)

    function currentPos() {
        if (Settings.taskbarPositions !== "") {
            try {
                const map = JSON.parse(Settings.taskbarPositions)
                if (map && map[String(screenIndex)] !== undefined)
                    return map[String(screenIndex)]
            } catch (e) {}
        }
        return Settings.taskbarWidgetPos || "tray"
    }
    function savePos(pos) {
        let map = {}
        try { map = Settings.taskbarPositions !== "" ? JSON.parse(Settings.taskbarPositions) : {} } catch (e) {}
        map[String(screenIndex)] = pos
        Settings.taskbarPositions = JSON.stringify(map)
        if (screenIndex === 0)
            Settings.taskbarWidgetPos = pos
    }

    function place() {
        if (shotPath !== "") {
            placement = 1
            return
        }
        const result = WindowShell.taskbarPlace(root, slotWidth, height, embed, currentPos())
        if (result < 0) {
            placement = 0
            root.lost()
            return
        }
        placement = result
    }
    Component.onCompleted: {
        WindowShell.makeToolWindow(root)
        place()
        _ready = true
    }
    Component.onDestruction: {
        WindowShell.keyHook(root, false)
        WindowShell.taskbarDetach(root)
    }

    Timer {
        interval: 2000
        repeat: true
        running: root._ready && root.shotPath === ""
        onTriggered: {
            if (!root.dragActive)
                root.place()
            root._fullscreen = root.placement === 2 && WindowShell.fullscreenOn(root)
        }
    }
    Connections {
        target: Settings
        function onTaskbarWidgetPosChanged() { if (!root.dragActive) root.place() }
        function onTaskbarPositionsChanged() { if (!root.dragActive) root.place() }
    }

    // Keys without focus: ← → tracks, ↑ ↓ volume, space
    function floatingKey(code) {
        switch (Number(code)) {
        case 1: Player.previous(); break
        case 2: Player.next(); break
        case 3: nudgeVolume(0.05); break
        case 4: nudgeVolume(-0.05); break
        case 5: Player.togglePlay(); break
        }
    }
    function nudgeVolume(d) {
        if (Player.remote === true && Player.canSetVolume === false)
            return
        Player.volume = Math.max(0, Math.min(1, Player.volume + d))
        coverVolume.show()
        volumeShown.restart()
    }
    Timer { id: volumeShown; interval: 1100 }

    function _time(ms) {
        const s = Math.max(0, Math.floor(ms / 1000))
        return Math.floor(s / 60) + ":" + String(s % 60).padStart(2, "0")
    }

    Item {
        id: content
        anchors.fill: parent

        HoverHandler {
            id: hover
            onHoveredChanged: WindowShell.keyHook(root, hovered)
        }
        WheelHandler {
            acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
            onWheel: event => {
                const touchpad = event.pixelDelta.y !== 0 && event.angleDelta.y % 120 !== 0
                root.nudgeVolume(touchpad ? event.pixelDelta.y / 600 : event.angleDelta.y / 120 * 0.05)
            }
        }
        TapHandler {
            acceptedButtons: Qt.RightButton
            onTapped: point => menu.openAt(Qt.point(point.position.x, point.position.y))
        }

        // ---- Pill ----
        Rectangle {
            id: pill
            anchors.fill: parent
            radius: height / 2
            clip: true
            color: root.colorMode ? Theme.primaryContainer : "#000000"
            border.width: 1
            border.color: Qt.rgba(1, 1, 1, root.colorMode ? 0.14 : 0.10)

            // Colored: the cover, blurred and "floating", like the player background in the app (background settings);
            // a mask crops it to the pill shape (clip cuts a rectangle)
            Item {
                id: coverFill
                anchors.fill: parent
                visible: root._onCover
                layer.enabled: visible
                layer.effect: MultiEffect {
                    maskEnabled: true
                    maskSource: coverMask
                    maskThresholdMin: 0.5
                    maskSpreadAtMin: 1.0
                }
                TrackBackdrop {
                    anchors.fill: parent
                    source: Player.cover
                    mode: Settings.backdropMode
                    blur: Settings.nowPlayingBlur
                    flow: Settings.nowPlayingDrift
                    running: root.motion && Player.playing
                    followTheme: Settings.accentFromCover
                }
                Rectangle {
                    anchors.fill: parent
                    color: Qt.rgba(0, 0, 0, 0.42)
                }
            }
            Rectangle {
                id: coverMask
                anchors.fill: parent
                visible: false
                layer.enabled: coverFill.visible
                radius: height / 2
            }
            // MD3 state layer: 8 % on hover
            Rectangle {
                anchors.fill: parent
                color: root.ink
                opacity: root.dragActive ? 0.12 : hover.hovered ? 0.08 : 0
                Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
            }
        }
        // an accent outline while dragging
        Rectangle {
            anchors.fill: parent
            radius: height / 2
            color: "transparent"
            border.width: 2
            border.color: Theme.primary
            opacity: root.dragActive ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
        }

        // ---- Cover and labels: click — open Materialfy, press and drag — move the widget ----
        Item {
            id: track
            anchors.left: parent.left
            anchors.leftMargin: 4
            anchors.right: buttons.left
            anchors.rightMargin: 4
            anchors.top: parent.top
            anchors.bottom: parent.bottom

            Item {
                id: cover
                width: 32
                height: 32
                anchors.verticalCenter: parent.verticalCenter
                scale: root.dragActive ? 0.92 : 1
                Behavior on scale { NumberAnimation { duration: Theme.motion.effectsFast } }
                MorphImage {
                    id: coverImage
                    anchors.fill: parent
                    visible: Player.cover !== ""
                    source: Player.cover
                    eager: true
                    fallbackKind: Player.album !== "" ? "album" : "track"
                    fallbackArtist: Player.artist.split(", ")[0]
                    fallbackTitle: Player.album !== "" ? Player.album : Player.title
                    // MD3E: paused — a soft square, playing — a "cookie"; on hover — a circle
                    shape: hover.hovered ? "circle" : "softSquare"
                    // as in the app: "Cover pulse" and "Frame rotation" from settings
                    pulse: root.wantsLevels ? Math.min(0.24, Player.level * 0.35) : 0
                    MaskSpin { target: coverImage; spinning: Settings.coverSpin && Player.playing && Player.isLocal === true && root.motion }
                }
                MorphShape {
                    anchors.fill: parent
                    visible: Player.cover === ""
                    shape: "softSquare"
                    color: Qt.alpha(root.ink, 0.14)
                    Icon { anchors.centerIn: parent; name: Player.isEpisode ? "podcasts" : "music_note"; size: 18; color: root.inkDim }
                }
                CoverVolume {
                    id: coverVolume
                    anchors.fill: parent
                    radius: hover.hovered || Player.playing ? width / 2 : width * 0.3
                    accent: root.accent
                }
            }
            Column {
                id: texts
                anchors.left: cover.right
                anchors.leftMargin: 9
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                anchors.verticalCenterOffset: -5
                spacing: -2
                Label {
                    width: parent.width
                    text: Player.title
                    type: "labelMedium"
                    weight: 600
                    color: root.ink
                    elide: Text.ElideRight
                }
                Label {
                    width: parent.width
                    text: volumeShown.running ? I18n.t("Volume {0}%", Math.round(Player.volume * 100))
                        : seekBar.scrubbing ? root._time(seekBar.shownValue * Player.durationMs) + " / " + root._time(Player.durationMs)
                        : Player.artist
                    type: "labelSmall"
                    weight: 400
                    color: root.inkDim
                    elide: Text.ElideRight
                }
            }
            // the progress wave under the labels: it does not run (the widget is always visible), it moves with the track position.
            // Click or drag — seek (over the "open / drag the widget" area); while hovered or
            // dragging, the artist is replaced by the time
            WavyProgress {
                id: seekBar
                readonly property bool scrubbing: dragging || seekHover.hovered
                anchors.left: texts.left
                anchors.right: parent.right
                anchors.rightMargin: 4
                anchors.top: texts.bottom
                height: 8
                z: 2
                thickness: scrubbing ? 4 : 3
                amplitude: 1.5
                wavelength: 12
                speed: 0
                gap: 3
                wavy: Player.playing && !scrubbing
                value: Player.position
                interactive: Player.durationMs > 0
                activeColor: root.accent
                stopColor: activeColor
                trackColor: Qt.alpha(root.ink, 0.32)
                onCommitted: v => Player.seek(v)
                Behavior on thickness { NumberAnimation { duration: Theme.motion.effectsFast } }
                HoverHandler { id: seekHover; margin: 4 }
            }

            MouseArea {
                id: grab
                anchors.fill: parent
                acceptedButtons: Qt.LeftButton
                cursorShape: root.dragActive ? Qt.ClosedHandCursor : Qt.PointingHandCursor
                preventStealing: true
                property point _start
                // dragging starts after moving the mouse 6 px or holding for 250 ms
                Timer { id: holdTimer; interval: 250; onTriggered: grab._beginDrag() }
                function _beginDrag() {
                    if (root.dragActive || !pressed || root.shotPath !== "")
                        return
                    root.dragActive = true
                    WindowShell.taskbarDrag(root, root.slotWidth, root.height, 0)
                }
                onPressed: mouse => {
                    _start = Qt.point(mouse.x, mouse.y)
                    holdTimer.restart()
                }
                onPositionChanged: mouse => {
                    if (!root.dragActive && Math.abs(mouse.x - _start.x) + Math.abs(mouse.y - _start.y) > 6)
                        _beginDrag()
                    if (root.dragActive)
                        WindowShell.taskbarDrag(root, root.slotWidth, root.height, 1)
                }
                onReleased: {
                    holdTimer.stop()
                    if (!root.dragActive) {
                        WindowShell.raiseMain()
                        return
                    }
                    const pos = WindowShell.taskbarDrag(root, root.slotWidth, root.height, 2)
                    root.dragActive = false
                    if (pos !== "")
                        root.savePos(pos)
                }
                onCanceled: {
                    holdTimer.stop()
                    if (root.dragActive)
                        WindowShell.taskbarDrag(root, root.slotWidth, root.height, 2)
                    root.dragActive = false
                    root.place()
                }
            }
        }

        // ---- Buttons: "back" slides in on hover, "play" is a filled MD3E shape, "next" ----
        Row {
            id: buttons
            anchors.right: parent.right
            anchors.rightMargin: 4
            anchors.verticalCenter: parent.verticalCenter
            spacing: 0
            BarButton {
                icon: "skip_previous"
                width: hover.hovered ? 30 : 0
                opacity: hover.hovered ? 1 : 0
                visible: width > 0.5
                clip: true
                Behavior on width { enabled: root.motion; NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
                Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
                onClicked: Player.previous()
            }
            PlayButton {
                anchors.verticalCenter: parent.verticalCenter
                size: 32
                playing: Player.playing
                loading: Player.buffering
                containerColor: root.accent
                iconColor: root.colorMode ? (root._onCover ? "#000000" : Theme.primaryContainer) : Theme.fgPrimaryFixed
                spin: false
                onClicked: Player.togglePlay()
            }
            BarButton {
                icon: "skip_next"
                onClicked: Player.next()
            }
        }
    }

    // Color change for a new cover — with the same curtain as the app window, and at the same time as it
    ThemeWipe {
        id: barWipe
        anchors.fill: parent
        source: content
        mode: Theme.reducedMotion ? "dissolve" : Settings.trackTransition
        originItem: coverImage
        Component.onCompleted: Theme.addWipe(barWipe)
        Component.onDestruction: Theme.removeWipe(barWipe)
    }

    FloatingMenu {
        id: menu
        anchors.fill: parent
        mainWindow: root.mainWindow
    }

    component BarButton: Item {
        id: button
        property string icon
        signal clicked()
        width: 30
        height: 30
        anchors.verticalCenter: parent ? parent.verticalCenter : undefined
        Rectangle {
            anchors.fill: parent
            radius: tap.pressed ? 9 : 15
            color: Qt.alpha(root.ink, tap.pressed ? 0.16 : bhover.hovered ? 0.1 : 0)
            Behavior on radius { NumberAnimation { duration: Theme.motion.effectsFast } }
        }
        Icon {
            anchors.centerIn: parent
            name: button.icon
            size: 20
            fill: 1
            color: root.ink
        }
        HoverHandler { id: bhover }
        TapHandler { id: tap; onTapped: button.clicked() }
    }
}
