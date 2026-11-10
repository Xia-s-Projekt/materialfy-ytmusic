import QtQuick
import QtQuick.Layouts
import QtQml.Models
import Qt.labs.platform as Platform
import Md3
import Materialfy.Core
import "app"

// Main window: sidebar | content (+ mini player) | right panel.
Window {
    id: win

    property string startPage: "home"     // --set startPage=settings | artist:<id> (for development)
    // --set autoPlay=<spotify uri> — start the context as soon as playback comes up; --set openNowPlaying=true —
    // then open "Now playing" (for documentation screenshots)
    property string autoPlay: ""
    property bool openNowPlaying: false
    // --set browse=album:<id>,artist:<id>,back,home,panel,sidebar… — one step every 4 s once the session is up:
    // open a page, go back, switch a section, toggle the right panel or the sidebar (memory measurements)
    property string browse: ""
    // --set floatingPlayer=island|taskbar|both|off — instead of the setting (shots); --set islandState=idle|peek|compact|
    // volume|announcement|expanded — the island's look; --set taskbarEmbed=false — the taskbar widget right above the panel, without embedding;
    // --set floatingShot=<path.png> — a shot of the island or widget window (also with --screenshot), --set taskbarTheme=dark|light
    property string floatingPlayer: ""
    property string islandState: ""
    property bool taskbarEmbed: true
    property string floatingShot: ""
    property string taskbarTheme: ""

    // size and position — the last saved ones (see _restoreGeometry); the window is shown already in place
    width: 1280
    height: 800
    // Below the normal layout the window goes "compact" (the sidebar becomes a sheet), smaller still — to the mini player
    // mini mode: the cover, title and "back / play / next" no longer fit
    minimumWidth: 400
    minimumHeight: 120
    visible: false
    title: Player.hasTrack ? Player.title + " — " + Player.artist + " · Materialfy YTMusic" : "Materialfy YTMusic"
    // "Always on top" (mini player button, track menu); not remembered between runs
    property bool pinnedOnTop: false
    // pinning is a property of mini mode: a window restored back is not always on top
    onLayoutModeChanged: if (layoutMode !== "mini") pinnedOnTop = false
    // Frames are drawn by the CPU (software rendering) — set by mf_shell.cpp, watch_software_rendering
    property bool softwareRendering: false
    // Wayland: there is no "always on top" protocol (GNOME and Weston ignore the flag), and the window does not know its coordinates
    readonly property bool wayland: Qt.platform.pluginName.indexOf("wayland") === 0
    readonly property bool canPin: !wayland
    // Player outside the window (Settings → Window → "Player above windows"): the island — Windows, macOS, X11 and KDE Wayland (there a KWin script places it: the window
    // cannot sit at the screen edge by itself), the taskbar — Windows only
    readonly property string floatingMode: {
        const mode = floatingPlayer !== "" ? floatingPlayer : Settings.floatingPlayer
        if (mode === "both" && Qt.platform.os === "windows")
            return mode
        if (mode === "island" && (!wayland || WindowShell.kwinAvailable()) && ["windows", "linux", "osx"].indexOf(Qt.platform.os) >= 0)
            return mode
        if (mode === "taskbar" && Qt.platform.os === "windows")
            return mode
        return "off"
    }
    flags: Qt.Window | Qt.FramelessWindowHint | (pinnedOnTop ? Qt.WindowStaysOnTopHint : 0)
    // Linux/macOS: the window is transparent, the background and rounded corners are drawn by WindowFrame (the window's alpha channel
    // is enabled by a color at creation, so the color does not change during operation)
    color: Qt.platform.os === "windows" ? Theme.surface : "transparent"

    // Layout by window size. A threshold with hysteresis: entering a mode at the threshold, leaving it 24 px further,
    // so the mode does not flip back and forth at the boundary. It switches both ways at once, right during
    // dragging the edge: the mode changes only when the threshold is crossed, not on every step.
    property string layoutMode: _modeFor(width, height, "")
    readonly property bool mini: layoutMode === "mini"
    function _modeFor(w, h, cur) {
        const band = 24
        const miniNow = cur === "mini" ? (w < 480 + band || h < 400 + band) : (w < 480 || h < 400)
        if (miniNow)
            return "mini"
        return (cur === "full" || cur === "") ? (w < 720 ? "compact" : "full") : (w < 720 + band ? "compact" : "full")
    }
    function _reevaluateMode() {
        const next = _modeFor(width, height, layoutMode)
        if (next !== layoutMode)
            layoutMode = next
    }
    // size and position — from settings; it also breaks the layoutMode binding (from there on — only _reevaluateMode)
    Component.onCompleted: _restoreGeometry()
    onWidthChanged: { _reevaluateMode(); if (_geoReady) geoSave.restart() }
    onHeightChanged: { _reevaluateMode(); if (_geoReady) geoSave.restart() }

    // 0 — the normal scene, 1 — the mini player; between them — a cross-fade with a slight scale
    property real miniProgress: mini ? 1 : 0
    Behavior on miniProgress {
        NumberAnimation { duration: 320; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized }
    }
    property bool _miniUsed: false
    onMiniChanged: {
        if (mini) {
            _miniUsed = true
            if (nowPlaying.open)
                nowPlaying.hide()
        }
    }

    readonly property bool panelFits: width >= 1100
    readonly property bool panelShown: Settings.rightPanelOpen && panelFits && layoutMode === "full"
    // An open right panel takes 340 px: in a window of 1400 or more the side panel collapses into a rail,
    // otherwise the content is left with less than in the narrowest window, and the mini player with the "orbit" does not fit
    readonly property bool compactSidebar: Settings.sidebarCollapsed || width < 980 || (panelShown && width < 1400)

    // Closing the window hides it in the tray (if that is enabled and the system has a tray) — the music keeps playing.
    // Quit — from the tray icon menu or Ctrl+Q.
    readonly property bool trayWanted: typeof materialfyTray !== "undefined" && materialfyTray && Settings.closeToTray
    readonly property bool trayReady: tray.object !== null && tray.object.available

    // From the mini player — back to the normal size (the last saved one, but no smaller than the normal layout)
    function expandFromMini() {
        if (visibility !== Window.Windowed)
            visibility = Window.Windowed
        const w = Math.max(Settings.windowWidth, 960), h = Math.max(Settings.windowHeight, 640)
        const s = screen
        x = Math.max(s.virtualX, Math.min(x, s.virtualX + s.width - w))
        y = Math.max(s.virtualY, Math.min(y, s.virtualY + s.height - h))
        width = w
        height = h
    }

    function showWindow() {
        show()
        raise()
        requestActivate()
    }

    // ---- Window size and position between runs ----
    property bool _geoReady: false
    function _onSomeScreen(px, py, w) {
        // at least the top of the window (the bar it can be dragged by) — on one of the monitors
        for (const s of Qt.application.screens) {
            if (px + w / 2 >= s.virtualX && px + w / 2 < s.virtualX + s.width && py >= s.virtualY && py + 24 < s.virtualY + s.height)
                return true
        }
        return false
    }
    function _restoreGeometry() {
        // --size (shots, debugging) beats the saved one
        const cliSize = Qt.application.arguments.some(a => a === "--size" || a.startsWith("--size="))
        if (!cliSize) {
            width = Math.max(minimumWidth, Settings.windowWidth)
            height = Math.max(minimumHeight, Settings.windowHeight)
        }
        if (wayland) {
            // on Wayland the compositor chooses the window position
        } else if (!cliSize && Settings.windowX > -1000000 && _onSomeScreen(Settings.windowX, Settings.windowY, width)) {
            x = Settings.windowX
            y = Settings.windowY
        } else {
            x = screen.virtualX + Math.max(0, (screen.width - width) / 2)
            y = screen.virtualY + Math.max(0, (screen.height - height) / 2)
        }
        layoutMode = _modeFor(width, height, "")
        if (!cliSize && Settings.windowMaximized)
            showMaximized()
        else
            show()
        _geoReady = true
    }
    function _saveGeometry() {
        if (!_geoReady)
            return
        if (visibility === Window.Maximized) {
            Settings.windowMaximized = true
            return
        }
        // minimized, fullscreen or hidden in the tray — the last normal position is left alone
        if (visibility !== Window.Windowed)
            return
        Settings.windowMaximized = false
        Settings.windowWidth = width
        Settings.windowHeight = height
        if (!wayland) {      // there x/y are always 0 — we do not overwrite the position saved under X11
            Settings.windowX = x
            Settings.windowY = y
        }
    }
    Timer { id: geoSave; interval: 400; onTriggered: win._saveGeometry() }
    onXChanged: if (_geoReady) geoSave.restart()
    onYChanged: if (_geoReady) geoSave.restart()
    onVisibilityChanged: if (_geoReady) geoSave.restart()

    onClosing: close => {
        _saveGeometry()
        if (trayWanted && trayReady) {
            close.accepted = false
            hide()
        } else {
            Qt.quit()
        }
    }

    Instantiator {
        id: tray
        active: win.trayWanted
        delegate: Platform.SystemTrayIcon {
            visible: true
            // a solid color icon per panel: Windows — the taskbar theme, otherwise the Qt theme; macOS — a template (mask)
            readonly property bool darkPanel: Settings.systemTaskbarTheme !== ""
                ? Settings.systemTaskbarTheme === "dark"
                : Qt.styleHints.colorScheme !== Qt.ColorScheme.Light
            readonly property real dpr: win.screen ? win.screen.devicePixelRatio : 1
            icon.source: Qt.resolvedUrl("../assets/tray-" + (darkPanel && Qt.platform.os !== "osx" ? "white" : "black")
                                        + "-" + (dpr >= 1.75 ? 32 : dpr >= 1.25 ? 24 : 16) + ".png")
            icon.mask: Qt.platform.os === "osx"
            tooltip: Player.hasTrack ? Player.title + " — " + Player.artist : "Materialfy YTMusic"
            onActivated: reason => {
                if (reason === Platform.SystemTrayIcon.MiddleClick)
                    Player.togglePlay()
                else if (reason === Platform.SystemTrayIcon.Context) {
                    // on Windows the tray menu is a widget, Qt does not attach it to the icon: we open it ourselves at the cursor
                    if (Qt.platform.os === "windows")
                        trayMenu.open()
                } else
                    win.visible ? win.hide() : win.showWindow()
            }
            menu: Platform.Menu {
                id: trayMenu
                Platform.MenuItem {
                    text: win.visible ? I18n.t("Hide window") : I18n.t("Show window")
                    onTriggered: win.visible ? win.hide() : win.showWindow()
                }
                Platform.MenuSeparator {}
                Platform.MenuItem {
                    text: Player.playing ? I18n.t("Pause") : I18n.t("Resume")
                    enabled: Player.hasTrack
                    onTriggered: Player.togglePlay()
                }
                Platform.MenuItem { text: I18n.t("Next track"); enabled: Player.hasTrack; onTriggered: Player.next() }
                Platform.MenuItem { text: I18n.t("Previous track"); enabled: Player.hasTrack; onTriggered: Player.previous() }
                Platform.MenuSeparator {}
                Platform.MenuItem { text: I18n.t("Quit"); onTriggered: Qt.quit() }
            }
        }
    }

    // All window content — the source of the shot for the theme change curtain (ThemeWipe on top, outside it)
    Item {
        id: scene
        anchors.fill: parent
        // in the mini player the scene is not unloaded (pages and scroll are preserved), it only dims and stops drawing
        visible: win.miniProgress < 1
        enabled: !win.mini
        // the departing layout fades faster than the new one appears
        opacity: Math.max(0, 1 - win.miniProgress * 1.6)
        scale: Theme.reducedMotion ? 1 : 1 - 0.04 * win.miniProgress
        layer.enabled: win.miniProgress > 0 && win.miniProgress < 1

        Rectangle { anchors.fill: parent; color: Theme.surface }

        // Atmospheric background: a blurred cover behind the whole window. The sidebar lies right on it,
        // the content panels are semi-transparent — the glow is weaker there.
        TrackBackdrop {
            id: ambient
            anchors.fill: parent
            visible: Settings.ambientBackground && Player.cover !== ""
            opacity: Theme.dark ? 0.3 : 0.35
            source: Player.cover
            // "glass" grain under the whole interface is unnecessary
            mode: Settings.backdropMode === "glass" ? "gauss" : Settings.backdropMode
            flow: Settings.nowPlayingDrift
            followTheme: Settings.accentFromCover
            running: visible && Player.playing && !nowPlaying.open
        }

        RowLayout {
            anchors.fill: parent
            anchors.margins: 8
            anchors.leftMargin: 0
            spacing: 8

            Sidebar {
                id: sidebar
                Layout.fillHeight: true
                Layout.preferredWidth: implicitWidth
                Behavior on Layout.preferredWidth { NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
                router: router
                compact: win.compactSidebar
                drawer: win.layoutMode !== "full"
                onNewPlaylistRequested: newPlaylistDialog.open(null)
            }

            // Content
            Surface {
                id: content
                Layout.fillWidth: true
                Layout.fillHeight: true
                level: "container"
                translucency: ambient.visible ? 0.3 : 0
                clip: true

                TopBar {
                    id: topBar
                    anchors.left: parent.left
                    anchors.right: parent.right
                    router: router
                    window: win
                    showMenuButton: win.layoutMode !== "full"
                    onMenuClicked: sidebar.openDrawer()
                    z: 2
                }

                Item {
                    id: pageHost
                    anchors.top: topBar.bottom
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.bottom: parent.bottom

                    Router {
                        id: router
                        // settings/releases/liked are built once and live: the first entry incubates
                        // (600–1200 ms), every later one is instant. Parameterized ones (playlists,
                        // artists) — LRU slots in keepAliveSlots.
                        keepAlive: ["home", "settings", "releases", "liked"]
                        keepAliveSlots: 6
                        pages: ({
                            home: Qt.resolvedUrl("app/pages/HomePage.qml"),
                            search: Qt.resolvedUrl("app/pages/SearchPage.qml"),
                            library: Qt.resolvedUrl("app/pages/LibraryPage.qml"),
                            liked: Qt.resolvedUrl("app/pages/LikedPage.qml"),
                            playlist: Qt.resolvedUrl("app/pages/PlaylistPage.qml"),
                            album: Qt.resolvedUrl("app/pages/AlbumPage.qml"),
                            artist: Qt.resolvedUrl("app/pages/ArtistPage.qml"),
                            show: Qt.resolvedUrl("app/pages/ShowPage.qml"),
                            episode: Qt.resolvedUrl("app/pages/EpisodePage.qml"),
                            radio: Qt.resolvedUrl("app/pages/RadioPage.qml"),
                            releases: Qt.resolvedUrl("app/pages/ReleasesPage.qml"),
                            settings: Qt.resolvedUrl("app/pages/SettingsPage.qml"),
                            account: Qt.resolvedUrl("app/pages/AccountPage.qml"),
                            playback: Qt.resolvedUrl("app/pages/PlaybackSetupPage.qml")
                        })
                        anchors.fill: parent
                        // startPage=artist:<id> (album:, playlist:, show:, episode:) — a page with a parameter. The catalog
                        // is read through the librespot session, so such a page opens once the session is up
                        Component.onCompleted: {
                            Qt.callLater(router._prewarm)   // before any return: works without a deep link too
                            const [name, id] = win.startPage.split(":")
                            if (!id) {
                                reset(name)
                                return
                            }
                            reset("home")
                            const open = () => {
                                if (["ready", "error", "premiumRequired"].indexOf(Auth.playbackState) < 0)
                                    return
                                Auth.playbackChanged.disconnect(open)
                                reset(name, ({ [name + "Id"]: id }))
                            }
                            Auth.playbackChanged.connect(open)
                            open()
                        }
                    }
                    Connections {
                        target: Auth
                        enabled: win.autoPlay !== ""
                        function onPlaybackChanged() {
                            if (Auth.playbackState !== "ready")
                                return
                            Player.playContext(win.autoPlay, "")
                            win.autoPlay = ""
                            if (win.openNowPlaying)
                                nowPlayingTimer.start()
                        }
                    }
                    Timer {
                        id: browseTimer
                        property var steps: win.browse !== "" ? win.browse.split(",") : []
                        interval: 4000
                        repeat: true
                        running: steps.length > 0 && ["ready", "error", "premiumRequired"].indexOf(Auth.playbackState) >= 0
                        onTriggered: {
                            const step = steps[0]
                            steps = steps.slice(1)
                            const [name, id] = step.split(":")
                            if (name === "back")
                                router.back()
                            else if (name === "panel")
                                Settings.rightPanelOpen = !Settings.rightPanelOpen
                            else if (name === "sidebar")
                                Settings.sidebarCollapsed = !Settings.sidebarCollapsed
                            else if (!id)
                                router.reset(name)
                            else
                                router.push(name, ({ [name + "Id"]: id }))
                        }
                    }
                    Timer {
                        id: nowPlayingTimer
                        interval: 4000
                        onTriggered: nowPlaying.show(Qt.point(win.width / 2, win.height - 60))
                    }
                }

                MiniPlayer {
                    id: miniPlayer
                    anchors.bottom: parent.bottom
                    anchors.bottomMargin: 16
                    anchors.horizontalCenter: parent.horizontalCenter
                    width: Math.min(parent.width - 32, 1080)
                    compact: win.layoutMode !== "full" || win.width < 860
                    z: 3
                    onOpenNowPlaying: center => nowPlaying.show(center)
                }
            }

            // Right panel
            Item {
                Layout.fillHeight: true
                Layout.preferredWidth: win.panelShown ? 340 : 0
                Layout.leftMargin: win.panelShown ? 0 : -8   // removes the RowLayout gap of a closed panel
                Behavior on Layout.preferredWidth { NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
                Behavior on Layout.leftMargin { NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
                clip: true

                // Created on first open: it is not needed when closed, and building it is expensive.
                // Closed is not just transparent, it is hidden: otherwise its animations (empty queue morphs, lyrics
                // text) run off-screen and redraw the whole window. (The Loader is hidden, not its place in RowLayout:
                // an invisible place is dropped by the layout along with the gap, and the page would become wider.)
                Loader {
                    width: 340
                    height: parent.height
                    visible: parent.width > 0
                    active: win.panelShown || item !== null
                    sourceComponent: RightPanel {
                        opacity: win.panelShown ? 1 : 0
                        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }
                    }
                }
            }
        }

        NowPlaying {
            id: nowPlaying
            anchors.fill: parent
            compact: win.layoutMode !== "full"
            z: 100
        }
    }

    Loader {
        id: miniMode
        anchors.fill: parent
        // a once-created mini player stays (no recreation on every squeeze), invisible — not drawn
        active: win.mini || win._miniUsed
        visible: win.miniProgress > 0
        enabled: win.mini
        opacity: Math.max(0, (win.miniProgress - 0.25) / 0.75)
        scale: Theme.reducedMotion ? 1 : 0.96 + 0.04 * win.miniProgress
        layer.enabled: win.miniProgress > 0 && win.miniProgress < 1
        sourceComponent: MiniMode {
            window: win
            onExpandRequested: win.expandFromMini()
        }
    }

    // Color change (and of the background — it changes under the shot) for a new cover. The method — from settings;
    // direction: forward in the queue — left to right, back — right to left; the wave travels from the cover
    ThemeWipe {
        id: themeWipe
        anchors.fill: parent
        source: win.mini && miniMode.item ? miniMode.item : scene
        // reduced motion: no curtains or waves — a plain fade
        mode: Theme.reducedMotion ? "dissolve" : Settings.trackTransition
        originItem: win.mini && miniMode.item ? miniMode.item.coverItem
                  : nowPlaying.open ? nowPlaying.coverItem : miniPlayer.coverItem
        z: 1000
        Component.onCompleted: Theme.wipe = themeWipe
    }
    Binding { target: Theme; property: "wipeDirection"; value: Player.direction }
    // Volume level — to the fullscreen player and mini mode (cover "breathing"), the island bars and the pulse
    // of the cover in the taskbar
    Binding {
        target: Player
        property: "levelsWanted"
        value: nowPlaying.wantsLevels || (islandWindow.object !== null && islandWindow.object.wantsLevels)
            || (taskbarWindow.object !== null && taskbarWindow.object.wantsLevels)
    }

    // ---- Player outside the window: it lives beside the main window, and while it is minimized or in the tray ----
    function screenIndicesFromSetting(settingVal, screenCount) {
        if (!settingVal || settingVal === "" || settingVal === "0")
            return [0]
        if (settingVal === "all") {
            const arr = []
            for (let i = 0; i < screenCount; ++i) arr.push(i)
            return arr
        }
        const parts = settingVal.split(",")
        const res = []
        for (const p of parts) {
            const idx = parseInt(p.trim(), 10)
            if (!isNaN(idx) && idx >= 0 && idx < screenCount && res.indexOf(idx) === -1)
                res.push(idx)
        }
        return res.length > 0 ? res : [0]
    }

    readonly property var activeIslandScreens: {
        const screens = Qt.application.screens
        const count = screens ? screens.length : 1
        // macOS: on all screens, but only the one with a notch will show it (FloatingIsland.qml)
        return win.screenIndicesFromSetting(Qt.platform.os === "osx" ? "all" : Settings.floatingIslandScreens, count)
    }

    readonly property var activeTaskbarScreens: {
        const screens = Qt.application.screens
        const count = screens ? screens.length : 1
        return win.screenIndicesFromSetting(Settings.taskbarScreens, count)
    }

    // A shot (--screenshot) captures only the main window — the player outside the window is not needed there (except floatingShot).
    // The island lives even without a track (a thin bar at the edge), the taskbar widget — only with a track
    readonly property bool _floatingAllowed: floatingShot !== "" || !Qt.application.arguments.some(a => a.startsWith("--screenshot"))

    Instantiator {
        id: islandWindow
        active: (win.floatingMode === "island" || win.floatingMode === "both") && win._floatingAllowed
        model: win.activeIslandScreens
        delegate: FloatingIsland {
            required property int modelData
            mainWindow: win
            screenIndex: modelData
            targetScreen: (Qt.application.screens && modelData < Qt.application.screens.length) ? Qt.application.screens[modelData] : null
            forcedState: win.islandState
            shotPath: win.floatingShot
        }
    }
    Instantiator {
        id: taskbarWindow
        active: (win.floatingMode === "taskbar" || win.floatingMode === "both") && win._floatingAllowed && Player.hasTrack
            && !_taskbarReset
        property bool _taskbarReset: false
        model: win.activeTaskbarScreens
        delegate: TaskbarPlayer {
            required property int modelData
            mainWindow: win
            screenIndex: modelData
            targetScreen: (Qt.application.screens && modelData < Qt.application.screens.length) ? Qt.application.screens[modelData] : null
            embed: win.taskbarEmbed
            shotPath: win.floatingShot
            forcedTheme: win.taskbarTheme
            // Explorer restarted and carried the window away — recreate it (not from the window's own handler)
            onLost: taskbarRecreate.start()
        }
    }
    Timer {
        id: taskbarRecreate
        interval: 0
        onTriggered: {
            taskbarWindow._taskbarReset = true
            taskbarWindow._taskbarReset = false
        }
    }
    Binding {
        target: Theme
        property: "lowPower"
        value: Settings.lowPowerMode === "on" || (Settings.lowPowerMode !== "off" && win.softwareRendering)
    }
    // Window not focused (or minimized) — decorative animations stop
    // a pinned mini player is on top of other windows — its animations do not freeze even without focus
    Binding {
        target: Theme; property: "calm"
        value: Settings.calmWhenInactive && (win.visibility === Window.Minimized
               || (!win.active && !(win.pinnedOnTop && win.layoutMode === "mini")))
    }

    // Navigation to artist / album / playlist — from anywhere (track rows, players)
    Connections {
        target: Catalog
        function onOpenRequested(name, props) {
            if (win.mini)
                win.expandFromMini()
            artistMenu.close()
            artistPreview.hide()
            if (nowPlaying.open)
                nowPlaying.hide()
            // a link search with search already open — the same screen with a new query
            const page = router.currentItem
            if (name === "search" && router.currentName === "search" && page && page.showQuery)
                page.showQuery(Catalog.searchQuery)
            else
                router.push(name, props)
        }
        function onPreviewRequested(id, x, y, w) { artistPreview.show(id, x, y, w) }
        function onPreviewCancelled() { artistPreview.hideSoon() }
        function onChooseArtist(refs) {
            artistMenu.refs = refs
            artistMenu.open(refs.map(r => ({ text: r.name, icon: "person" })), cursor.point.position)
        }
        function onTrackMenuRequested(track, x, y, w, h) {
            artistPreview.hide()
            // "Remove from this playlist" — only on your own playlist page
            const page = router.currentItem
            trackMenu.playlistId = !nowPlaying.open && router.currentName === "playlist" && page && page.canEdit ? page.playlistId : ""
            trackMenu.show(track, Qt.point(x, y), w > 0 && h > 0 ? Qt.size(w, h) : undefined)
        }
    }
    // Short service messages; "did not respond" only ever comes from Player
    Connections {
        target: Library
        function onNotice(text) { snackbar.show(text) }
    }
    Connections {
        target: Player
        ignoreUnknownSignals: true
        function onNotice(text) { snackbar.show(text) }
    }
    Connections {
        target: Devices
        ignoreUnknownSignals: true
        function onNotice(text) { snackbar.show(text) }
    }
    // Snackbar for pages: win.notify(text[, action, function])
    function notify(text, actionText, onAction) {
        snackbar.show(text, actionText ?? "", onAction ?? null)
    }

    // "Radio by track / artist": the station plays immediately, with no navigation and no question; the seed is selected and on the
    // home page's orbit, so it is visible there what is playing. An empty title — the service supplies the label.
    function radioFrom(seedUri, title) {
        Radio.select(seedUri)
        Radio.playStation(seedUri, title)
    }

    // Settings on the right section (island and taskbar widget menu)
    function openSettings(section) {
        if (nowPlaying.open)
            nowPlaying.hide()
        if (win.mini)
            win.expandFromMini()
        router.reset("settings", section ? { section: section } : undefined)
    }

    function trackMenuAction(id, track, item) {
        // the queue, settings and navigation live in the normal layout
        if (win.mini && (id === "queue" || id === "sound" || id === "removefromplaylist"))
            win.expandFromMini()
        if (id === "radio:track") {
            win.radioFrom("ytmusic:track:" + track.trackId, track.title ?? "")
        } else if (id.startsWith("radio:artist:")) {
            const artistId = id.slice("radio:artist:".length)
            const ref = (track.artistRefs ?? []).find(r => r.id === artistId)
            win.radioFrom("ytmusic:artist:" + artistId, ref ? ref.name : (track.artists ?? ""))
        } else if (id === "addqueue") {
            Player.addToQueue(track)
            snackbar.show(I18n.t("Added to queue"))
        } else if (id === "dislike") {
            Library.dislike(track)
        } else if (id === "like") {
            Library.toggleLike(track)
        } else if (id === "save") {
            const uri = track.isEpisode ? (track.uri ?? "") : "ytmusic:album:" + (track.albumId ?? "")
            const was = Library.isSaved(uri)
            Library.toggleSaved(uri)
            snackbar.show(was ? I18n.t("Removed from library") : I18n.t("Saved to library"))
        } else if (id === "removefromplaylist") {
            const page = router.currentItem
            if (router.currentName === "playlist" && page && typeof track.pos === "number")
                page.trackRemoved(track.pos)
        } else if (id === "repeat") {
            Player.cycleRepeat()
        } else if (id === "queue") {
            if (nowPlaying.open)
                nowPlaying.hide()
            Settings.rightPanelTab = "queue"
            Settings.rightPanelOpen = true
        } else if (id === "sound") {
            if (nowPlaying.open)
                nowPlaying.hide()
            router.reset("settings", { section: "playback" })
        } else if (id === "playlist:new") {
            newPlaylistDialog.open(track)
        } else if (id.startsWith("playlist:")) {
            Library.addToPlaylist(track, id.slice("playlist:".length))
        } else if (id === "share:track") {
            Catalog.copyLink(Catalog.trackUrl(track))
            snackbar.show(I18n.t("Link copied"))
        } else if (id.startsWith("share:artist:")) {
            Catalog.copyLink(Catalog.artistUrl(id.slice("share:artist:".length)))
            snackbar.show(I18n.t("Link copied"))
        } else if (id === "goto:track") {
            Catalog.openAlbum(track.albumId, track.trackId)
        } else if (id === "goto:album") {
            Catalog.openAlbum(track.albumId)
        } else if (id === "goto:artist") {
            Catalog.openArtists(track.artistRefs ?? [])
        } else if (id === "goto:show") {
            Catalog.openShow(track.showId ?? "")
        }
    }
    HoverHandler { id: cursor }
    ArtistPreview {
        id: artistPreview
        anchors.fill: parent
        z: 1900
    }
    Menu {
        id: artistMenu
        property var refs: []
        anchors.fill: parent
        z: 2000
        onPicked: i => Catalog.openArtist(refs[i].id)
    }
    TrackMenu {
        id: trackMenu
        z: 2000
        onTriggered: (id, track, item) => win.trackMenuAction(id, track, item)
    }
    NewPlaylistDialog {
        id: newPlaylistDialog
        z: 2100
    }
    // YTMusic fork: no Spotify Web API client-ID dialog — sign-in is headers-based.
    Loader {
        z: 2120
        active: false
        source: Qt.resolvedUrl("app/ApiLimitDialog.qml")
    }
    UpdateSheet {
        z: 2150
    }
    Snackbar {
        id: snackbar
        anchors.fill: parent
        z: 2200
    }
    // The last run crashed or hung (new entries in crash.log) — offer to report it, once
    Timer {
        interval: 2500
        running: CrashReport.pending
        onTriggered: {
            snackbar.show(I18n.t("Materialfy closed unexpectedly last time"), I18n.t("Report"), () => {
                Qt.openUrlExternally(CrashReport.folder)
                Qt.openUrlExternally(CrashReport.issueUrl)
            }, 20000)
            CrashReport.dismiss()
        }
    }

    ResizeEdges { window: win }
    // takes all window content to itself: the window background and corner rounding (see WindowFrame)
    WindowFrame { window: win }

    Loader {
        active: typeof materialfyDebugFps !== "undefined" && materialfyDebugFps
        anchors.top: parent.top
        anchors.right: parent.right
        anchors.margins: 16
        z: 10000
        sourceComponent: FpsOverlay {}
    }

    // Mouse "back" navigation and hotkeys
    TapHandler {
        acceptedButtons: Qt.BackButton
        onTapped: nowPlaying.open ? nowPlaying.hide() : router.back()
    }
    Shortcut { sequences: [StandardKey.Back, "Alt+Left"]; onActivated: nowPlaying.open ? nowPlaying.hide() : router.back() }

    // Player (space is not intercepted while focus is in an input field)
    Shortcut { sequence: "Space"; onActivated: Player.togglePlay() }
    Shortcut { sequences: ["Ctrl+Right", "Media Next"]; onActivated: Player.next() }
    Shortcut { sequences: ["Ctrl+Left", "Media Previous"]; onActivated: Player.previous() }
    Shortcut { sequence: "Ctrl+Up"; onActivated: Player.volume = Math.min(1, Player.volume + 0.05) }
    Shortcut { sequence: "Ctrl+Down"; onActivated: Player.volume = Math.max(0, Player.volume - 0.05) }
    Shortcut { sequence: "Shift+Right"; onActivated: Player.seekMs(Player.positionMs + 10000) }
    Shortcut { sequence: "Shift+Left"; onActivated: Player.seekMs(Player.positionMs - 10000) }
    Shortcut {
        sequence: "Ctrl+P"
        onActivated: nowPlaying.open ? nowPlaying.hide() : !win.mini && Player.hasTrack && nowPlaying.show(null)
    }
    Shortcut {
        sequences: [StandardKey.Find]
        onActivated: {
            // in the track list — search by it first; pressing again — a general search
            if (!nowPlaying.open && router.currentItem && router.currentItem.findInPage && router.currentItem.findInPage())
                return
            if (nowPlaying.open)
                nowPlaying.hide()
            // Search is already open: a dedup reset will not rebuild the page and focus will not happen on its own — we call it explicitly.
            if (router.currentName === "search" && router.currentItem && router.currentItem.focusSearch) {
                router.currentItem.focusSearch()
                return
            }
            router.reset("search")
            // SearchPage takes focus itself in Component.onCompleted — during incubation reset's currentItem is still the old one
        }
    }
    Shortcut { sequence: "Ctrl+,"; onActivated: router.reset("settings") }
    Shortcut { sequence: "Ctrl+Shift+L"; onActivated: router.reset("library") }
    Shortcut { sequence: "Ctrl+B"; onActivated: Settings.sidebarCollapsed = !Settings.sidebarCollapsed }
    Shortcut { sequence: "Ctrl+L"; onActivated: { Settings.rightPanelTab = "lyrics"; Settings.rightPanelOpen = !Settings.rightPanelOpen } }
    Shortcut { sequences: [StandardKey.Quit]; onActivated: Qt.quit() }
    // F11: fullscreen and back — to whatever the window was (maximized or normal)
    property bool _maximizedBeforeFullScreen: false
    Shortcut {
        sequence: "F11"
        onActivated: {
            if (win.visibility === Window.FullScreen) {
                win.visibility = win._maximizedBeforeFullScreen ? Window.Maximized : Window.Windowed
            } else {
                win._maximizedBeforeFullScreen = win.visibility === Window.Maximized
                win.visibility = Window.FullScreen
            }
        }
    }
}
