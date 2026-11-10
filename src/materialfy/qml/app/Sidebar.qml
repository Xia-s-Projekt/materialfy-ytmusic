import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Left navigation. compact — a rail of icons (narrow window or collapsed manually).
//
// The geometry is the same for both modes: 12 px margins, all icons (the mark, sections, playlist covers, avatar)
// are centered on the x = 40 axis — the center of an 80-wide rail. On collapse only the width animates
// (driven by Main.qml), the labels fade out, heights do not change — so nothing jumps.
Item {
    id: root

    required property var router
    property bool compact: false
    readonly property bool signedIn: Auth.state === "signedIn"
    signal newPlaylistRequested()
    // Drawer (narrow window): takes no width in the layout, on openDrawer() slides out over the content
    property bool drawer: false
    // Open over the content: a drawer or an expanded rail, when the rail is forced by the window width
    property bool drawerOpen: false
    // Rail due to the window width (not collapsed manually): the menu button opens the panel over the content
    readonly property bool forcedRail: compact && !drawer && !Settings.sidebarCollapsed
    // It closes with an animation — the drawer card is still visible until it ends
    property bool closing: false
    // Switching to the drawer: the layout space shrinks with an animation (Main.qml), and the rail slides left with it
    readonly property bool collapsing: drawer && !drawerOpen && !closing && width > 0.5
    // Panel over the content (drawer card, dimming, its own width of 264)
    readonly property bool floating: drawer ? !collapsing : (drawerOpen || closing)
    readonly property bool rail: (compact && !drawer && !drawerOpen) || collapsing

    function openDrawer() { closeTimer.stop(); closing = false; drawerOpen = true; panel.forceActiveFocus() }
    function closeDrawer() {
        if (!drawerOpen)
            return
        closing = true
        drawerOpen = false
        closeTimer.restart()
    }
    function go(name, props) { closeDrawer(); router.reset(name, props) }
    // Row menu (right-click): pin/unpin in the "Library". Pinned ones come first in the lists.
    property var _menuEntry: null
    function openRowMenu(entry, point) {
        _menuEntry = entry
        const uri = entry.uri ?? ""
        if (uri === "")
            return
        const pinned = Library.pinsRevision >= 0 && Library.isPinned(uri)
        rowMenu.open([{ id: "pin", text: pinned ? I18n.t("Unpin") : I18n.t("Pin to top"), icon: "push_pin" }],
                     rowMenu.mapFromItem(null, point.x, point.y))
    }
    function _menuAction(id) {
        const entry = _menuEntry
        if (!entry || id !== "pin")
            return
        const uri = entry.uri ?? ""
        const was = Library.isPinned(uri)
        Library.togglePin(uri)
        const w = root.Window.window
        if (w && typeof w.notify === "function")
            w.notify(was ? I18n.t("Unpinned") : I18n.t("Pinned — in this app only"))
    }

    // Sidebar row: playlist/folder — as an app page, the rest — as a catalog card
    function openEntry(entry) {
        const kind = entry.kind ?? ""
        closeDrawer()
        if (kind === "dj")
            // A DJ is not a playlist: it has no page with a track list — only a launch
            Player.playContext(entry.uri ?? "", "")
        else if (kind === "liked")
            router.reset("liked")
        else if (kind === "playlist")
            router.reset("playlist", { playlistId: entry.id, playlistTitle: entry.title ?? "", cover: entry.cover ?? "" })
        else if (kind === "album")
            Catalog.openAlbum(entry.id)
        else if (kind === "artist")
            Catalog.openArtist(entry.id)
        else if (kind === "show")
            Catalog.openShow(entry.id)
    }
    onDrawerChanged: { drawerOpen = false; closing = false }
    onCompactChanged: if (!compact && !drawer) { drawerOpen = false; closing = false }
    Timer { id: closeTimer; interval: Theme.motion.spatialDefault + 40; onTriggered: root.closing = false }

    readonly property real expandedWidth: 264
    readonly property real railWidth: 80
    readonly property real margin: 12
    readonly property real iconInset: 14     // icon square of 28 px: 12 + 14 + 14 = 40 — the rail axis
    readonly property real labelWidth: expandedWidth - 2 * margin - iconInset - 28 - 8 - 12

    readonly property var sections: [
        { name: "home", icon: "home", text: I18n.t("Home") },
        { name: "search", icon: "search", text: I18n.t("Search") },
        { name: "releases", icon: "new_releases", text: I18n.t("New releases") },
        { name: "library", icon: "library_music", text: I18n.t("Library") },
        { name: "liked", icon: "favorite", text: I18n.t("Liked Songs") }
    ]

    // Top-level playlists and folders from rootlist (Spotify order; pinned ones first, placed by the service).
    // An expanded folder (arrow on the right) shows its playlists and nested folders right in the list, indented;
    // a click on the folder itself opens it as a page (FolderPage). Expanded folders are remembered while the app is open.
    // "Library" mode: instead of the playlist tree — the most recently added/updated rows of all
    // sections (playlists, albums, artists, podcasts and "Liked"). This summary has no folders.
    readonly property bool libraryMode: Settings.sidebarLibrary === true
    readonly property var _recent: signedIn && libraryMode ? Library.recentEntries(40) : null
    readonly property var _source: signedIn && !libraryMode ? Library.items("playlists", "all", "spotify") : null
    property var playlists: []
    property var recentItems: []
    function _rebuild() {
        if (libraryMode) {
            const rows = []
            const recent = _recent
            for (let i = 0; recent && i < recent.count; i++)
                rows.push(Object.assign({}, recent.get(i), { depth: 0 }))
            recentItems = rows
            return
        }
        const out = []
        const m = _source
        for (let i = 0; m && i < m.count; i++) {
            const e = m.get(i)
            if (e.kind === "liked")
                continue
            out.push(Object.assign({}, e, { depth: 0 }))
        }
        playlists = out
    }
    on_SourceChanged: _rebuild()
    // on entry the model appears before Connections switches to it
    on_RecentChanged: _rebuild()
    onRailChanged: _rebuild()
    Connections {
        target: root._source
        ignoreUnknownSignals: true
        function onCountChanged() { root._rebuild() }
        function onModelReset() { root._rebuild() }
        function onDataChanged() { root._rebuild() }
    }
    Connections {
        target: root._recent
        ignoreUnknownSignals: true
        function onCountChanged() { root._rebuild() }
        function onModelReset() { root._rebuild() }
        function onDataChanged() { root._rebuild() }
    }
    // the mode was switched in settings — the list changes entirely
    onLibraryModeChanged: _rebuild()
    // folder contents change with the tree (covers, names, reordering in Spotify)
    Connections {
        target: Library
        function onEntriesChanged() {
            root._rebuild()
        }
        // a row was pinned/unpinned — the summary is rearranged
        function onPinsChanged() { if (root.libraryMode) root._rebuild() }
    }

    implicitWidth: drawer ? 0 : compact ? railWidth : expandedWidth
    clip: !drawer && !floating
    z: drawer || floating ? 50 : 0

    component FadeLabel: Label {
        opacity: root.rail ? 0 : 1
        visible: opacity > 0
        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
    }

    // The dimming under the drawer covers the whole window (coordinates are recomputed on open)
    Rectangle {
        id: scrim
        property point origin: Qt.point(0, 0)
        visible: opacity > 0
        x: -origin.x
        y: -origin.y
        width: root.Window.width
        height: root.Window.height
        color: Theme.scrim
        opacity: root.drawerOpen ? 0.32 : 0
        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }
        MouseArea { anchors.fill: parent; acceptedButtons: Qt.AllButtons; onClicked: root.closeDrawer(); onWheel: w => w.accepted = true }
    }
    onDrawerOpenChanged: if (drawerOpen) scrim.origin = root.mapToItem(null, 0, 0)

    Item {
        id: panel
        width: root.drawer ? (root.collapsing ? root.railWidth : root.expandedWidth)
             : root.drawerOpen ? root.expandedWidth : root.width
        height: root.height
        x: !root.drawer ? 0 : root.drawerOpen ? 0 : root.collapsing ? root.width - root.railWidth : -width - 24
        visible: !root.drawer || root.drawerOpen || root.collapsing || root.closing
        clip: root.floating && !root.drawer
        // the drawer slides out from the left; the expanded rail grows wider from the rail (MD3: rail -> drawer)
        Behavior on x {
            enabled: root.drawer && (root.drawerOpen || root.closing)
            NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedDecelerate }
        }
        Behavior on width {
            enabled: !root.drawer && root.compact
            NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized }
        }
        Keys.onEscapePressed: root.closeDrawer()

        // Empty spots of the panel (around the logo, between items, below the playlists) drag the window, like the top
        // bar; a double click — expand. The items, scrolling and the account card take the click themselves.
        DragHandler {
            property bool manual: false
            target: null
            grabPermissions: PointerHandler.TakeOverForbidden
            onActiveChanged: if (active) manual = !root.Window.window.startSystemMove()
            onCentroidChanged: if (active && manual && root.Window.window.visibility === Window.Windowed) {
                root.Window.window.x += centroid.scenePosition.x - centroid.scenePressPosition.x
                root.Window.window.y += centroid.scenePosition.y - centroid.scenePressPosition.y
            }
        }
        TapHandler {
            acceptedButtons: Qt.LeftButton
            onDoubleTapped: {
                const w = root.Window.window
                w.visibility = w.visibility === Window.Maximized ? Window.Windowed : Window.Maximized
            }
        }

        // Drawer card: rounded right edge (MD3 modal navigation drawer)
        Rectangle {
            id: drawerBg
            visible: root.floating
            anchors.fill: parent
            anchors.leftMargin: -Theme.shape.extraLarge
            radius: Theme.shape.extraLarge
            color: Theme.surfaceContainerLow
        }
        Elevation { target: drawerBg; level: 1 }
        MouseArea { visible: root.floating; anchors.fill: parent; acceptedButtons: Qt.AllButtons; onWheel: w => w.accepted = true }

    ColumnLayout {
        anchors.fill: parent
        anchors.topMargin: 12
        anchors.bottomMargin: 12
        anchors.leftMargin: root.margin
        anchors.rightMargin: root.margin
        spacing: 2

        // Header: the "Wave-M" mark on the rail axis + a label. The mark is the collapse / expand toggle
        // (hovering turns it into a button); in a narrow window — open the panel over the content.
        Item {
            Layout.fillWidth: true
            Layout.preferredHeight: 48
            Layout.bottomMargin: 8

            SidebarLogo {
                objectName: "sidebarLogo"
                x: 40 - root.margin - markSize / 2
                anchors.verticalCenter: parent.verticalCenter
                markSize: 40
                expanded: !root.rail
                onActivated: {
                    if (root.drawer || root.drawerOpen)
                        root.closeDrawer()
                    else if (root.forcedRail)
                        root.openDrawer()
                    else
                        Settings.sidebarCollapsed = !Settings.sidebarCollapsed
                }
            }
        }

        Repeater {
            model: root.sections
            NavItem {
                required property var modelData
                Layout.fillWidth: true
                compact: root.rail
                iconInset: root.iconInset
                labelWidth: root.labelWidth
                icon: modelData.icon
                text: modelData.text
                // before sign-in the sections are inactive (38%): there is nothing to look at in them; "Home" — with an invitation to sign in
                enabled: root.signedIn || modelData.name === "home"
                opacity: enabled ? 1 : Theme.stateLayer.disabledContent
                // "Library" is not highlighted while a folder is open inside it — the folder item highlights it
                selected: root.router.rootName === modelData.name
                          && !(modelData.name === "library" && (root.router.current.props.folderId ?? "") !== "")
                onClicked: root.go(modelData.name)
            }
        }

        // Section header: the label fades out, the wavy divider remains in the rail too
        Item {
            id: sectionHeader
            Layout.fillWidth: true
            Layout.topMargin: 12
            Layout.preferredHeight: 32

            FadeLabel {
                id: sectionLabel
                x: root.iconInset
                anchors.verticalCenter: parent.verticalCenter
                text: root.libraryMode ? I18n.t("Recently played") : I18n.t("Playlists")
                type: "labelMedium"
                color: Theme.fgSurfaceVariant
            }
            WavyDivider {
                // in "Library" mode and in the rail there is no "+" button — the line runs to the very edge
                x: root.rail ? root.iconInset : sectionLabel.x + sectionLabel.implicitWidth + 10
                width: parent.width - x - (root.rail || root.libraryMode ? 0 : 44)
                anchors.verticalCenter: parent.verticalCenter
                Behavior on x { NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
            }
            IconButton {
                anchors.right: parent.right
                anchors.verticalCenter: parent.verticalCenter
                icon: "add"
                size: "xs"
                iconColor: Theme.primary
                enabled: root.signedIn
                onClicked: root.newPlaylistRequested()
                // in "Library" mode the list is not of playlists: the "new playlist" button is out of place here
                opacity: root.rail || root.libraryMode ? 0 : 1
                visible: opacity > 0
                Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
            }
        }

        FadeLabel {
            visible: opacity > 0 && !root.signedIn
            Layout.preferredWidth: root.labelWidth + 28 + 8
            Layout.leftMargin: root.iconInset
            Layout.topMargin: 4
            text: root.libraryMode ? I18n.t("Sign in to see your library") : I18n.t("Sign in to see your playlists")
            type: "bodySmall"
            color: Theme.fgSurfaceVariant
            wrapMode: Text.WordWrap
            elide: Text.ElideNone
            lineHeightMode: Text.ProportionalHeight
            lineHeight: 1.2
        }
        FadeLabel {
            visible: opacity > 0 && root.signedIn && !root.libraryMode
                     && root._source !== null && !root._source.loading && root.playlists.length === 0
            Layout.preferredWidth: root.labelWidth + 28 + 8
            Layout.leftMargin: root.iconInset
            Layout.topMargin: 4
            text: I18n.t("No playlists yet. Create one with +")
            type: "bodySmall"
            color: Theme.fgSurfaceVariant
            wrapMode: Text.WordWrap
            elide: Text.ElideNone
            lineHeightMode: Text.ProportionalHeight
            lineHeight: 1.2
        }
        LoadingIndicator {
            visible: root.signedIn && !root.libraryMode
                     && root._source !== null && root._source.loading && root.playlists.length === 0
            Layout.leftMargin: 40 - root.margin - size / 2
            Layout.topMargin: 8
            size: 32
        }
        FadeLabel {
            // the "Library" summary is empty and already loaded: while loading, emptiness means nothing
            visible: opacity > 0 && root.signedIn && root.libraryMode && root.recentItems.length === 0
                     && root._recent !== null && root._recent.loading !== true
            Layout.preferredWidth: root.labelWidth + 28 + 8
            Layout.leftMargin: root.iconInset
            Layout.topMargin: 4
            text: I18n.t("Nothing in your library yet")
            type: "bodySmall"
            color: Theme.fgSurfaceVariant
            wrapMode: Text.WordWrap
            elide: Text.ElideNone
            lineHeightMode: Text.ProportionalHeight
            lineHeight: 1.2
        }

        // The user playlists (or the "Library" summary): their own scrolling, if there are many rows
        ListView {
            id: playlistList
            visible: root.signedIn && count > 0
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.topMargin: 4
            clip: true
            spacing: 2
            boundsBehavior: Flickable.StopAtBounds
            acceptedButtons: Qt.NoButton
            model: root.libraryMode ? root.recentItems : root.playlists
            delegate: NavItem {
                id: row
                required property var modelData
                readonly property string kind: modelData.kind ?? ""
                readonly property real indent: 0
                width: ListView.view.width
                compact: root.rail
                iconInset: root.iconInset
                labelWidth: root.labelWidth
                text: modelData.title ?? ""
                icon: kind === "album" ? "album"
                      : kind === "artist" ? "person"
                      : kind === "show" ? "podcasts"
                      : kind === "liked" ? "favorite"
                      : kind === "dj" ? "graphic_eq" : "queue_music"
                imageSource: modelData.cover ?? ""
                trailingIcon: modelData.pinned === true ? "push_pin" : ""
                trailingColor: Theme.primary
                selected: kind === "playlist"
                          ? root.router.currentName === "playlist" && (root.router.current.props.playlistId ?? "") === modelData.id
                          : kind === "album" ? root.router.currentName === "album" && (root.router.current.props.albumId ?? "") === modelData.id
                          : kind === "artist" ? root.router.currentName === "artist" && (root.router.current.props.artistId ?? "") === modelData.id
                          : kind === "show" ? root.router.currentName === "show" && (root.router.current.props.showId ?? "") === modelData.id
                          : kind === "liked" ? root.router.rootName === "liked" : false
                onClicked: root.openEntry(modelData)
                onRightClicked: point => root.openRowMenu(modelData, point)
            }
            SmoothScroll { flickable: playlistList; wheelStep: Settings.wheelStep; showScrollBar: false }
        }

        Item { Layout.fillHeight: true; visible: !playlistList.visible }

        // An available update and the download progress (the update window — UpdateSheet in Main.qml)
        UpdatePill {
            Layout.fillWidth: true
            rail: root.rail
            onClicked: { root.closeDrawer(); Updater.showSheet() }
        }

        // In the rail, settings are a separate item; when expanded — a button in the account card
        NavItem {
            visible: root.rail
            Layout.fillWidth: true
            Layout.topMargin: 8
            compact: root.rail
            iconInset: root.iconInset
            labelWidth: root.labelWidth
            icon: "settings"
            text: I18n.t("Settings")
            selected: root.router.rootName === "settings"
            onClicked: root.go("settings")
        }

        // Account card: the avatar on the rail axis, the same height in both modes
        Item {
            Layout.fillWidth: true
            Layout.topMargin: 8
            Layout.preferredHeight: 64

            Rectangle {
                anchors.fill: parent
                radius: Theme.shape.large
                color: root.router.rootName === "account" || !root.signedIn ? Theme.secondaryContainer : Theme.surfaceContainer
                Behavior on color { enabled: !Theme.transitioning; ColorAnimation { duration: Theme.motion.effectsDefault } }
            }

            Avatar {
                id: avatar
                x: 40 - root.margin - size / 2
                anchors.verticalCenter: parent.verticalCenter
                size: 40
                name: root.signedIn ? (Auth.account.displayName ?? "") : ""
                source: root.signedIn ? (Auth.account.avatar ?? "") : ""
            }

            Column {
                x: avatar.x + avatar.width + 12
                anchors.verticalCenter: parent.verticalCenter
                width: root.expandedWidth - 2 * root.margin - x - 12 - settingsButton.width
                opacity: root.rail ? 0 : 1
                visible: opacity > 0
                Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
                Label {
                    width: parent.width
                    text: root.signedIn ? (Auth.account.displayName ?? "") : I18n.t("Sign in")
                    type: "titleSmall"
                    color: root.signedIn ? Theme.fgSurface : Theme.fgSecondaryContainer
                }
                Label {
                    width: parent.width
                    text: root.signedIn ? I18n.t("YouTube Music")
                        : Auth.state === "checking" || Auth.state === "connecting" ? I18n.t("Signing in…")
                        : I18n.t("YouTube Music account")
                    type: "bodySmall"
                    color: Theme.fgSurfaceVariant
                }
            }

            StateLayer {
                anchors.fill: parent
                radius: Theme.shape.large
                onClicked: root.go("account")
            }

            IconButton {
                id: settingsButton
                anchors.right: parent.right
                anchors.rightMargin: 12
                anchors.verticalCenter: parent.verticalCenter
                icon: "settings"
                checkable: true
                autoToggle: false
                checked: root.router.rootName === "settings"
                opacity: root.rail ? 0 : 1
                visible: opacity > 0
                Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
                onClicked: root.go("settings")
            }
        }
    }
    }

    // Right-click on a row: pin/unpin
    Menu {
        id: rowMenu
        anchors.fill: parent
        z: 60
        onTriggered: (id, item) => root._menuAction(id)
    }
}
