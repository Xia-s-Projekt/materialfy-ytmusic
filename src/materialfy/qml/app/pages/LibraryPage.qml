import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core
import ".."

// Library: sections as chips (Playlists - Albums - Artists - Podcasts), playlists have an owner
// (All - Mine - Following - Collaborative); grid or list (Settings.libraryView), sorting
// (Settings.librarySort), local filter, rootlist folders (read-only), local pins.
// The grid is virtualized: a ListView of rows of cols tiles; "Liked tracks" is the first tile, two cells wide.
Item {
    id: page

    property var router
    readonly property string title: I18n.t("Library")
    readonly property bool showHeader: false
    readonly property bool scrolled: list.contentY - list.originY > 8

    property string kind: "playlists"         // playlists | albums | artists | shows
    property string owner: "all"              // all | mine | followed | collab
    property bool searchOpen: false
    property string query
    readonly property bool signedIn: Auth.state === "signedIn"
    readonly property bool grid: Settings.libraryView !== "list"
    readonly property bool showLiked: kind === "playlists" && owner === "all" && query.trim() === ""

    readonly property var model: !signedIn ? null
        : Library.items(kind, owner, Settings.librarySort)

    component Head: Label {
        property bool active: false
        type: "labelMedium"
        color: active ? Theme.primary : Theme.fgSurfaceVariant
    }

    readonly property var sorts: [
        { id: "recent", text: I18n.t("Recents") },
        { id: "added", text: I18n.t("Recently played") },
        { id: "alpha", text: I18n.t("Alphabetical") },
        { id: "author", text: I18n.t("Creator") },
        { id: "ytmusic", text: I18n.t("YouTube Music order") }
    ]
    readonly property string sortText: (sorts.find(s => s.id === Settings.librarySort) ?? sorts[0]).text

    // ---- Items: model -> array (text filter, without "Liked" — they have their own tile) ----
    property var entries: []
    function _rebuild() {
        const out = []
        const m = page.model
        const q = page.query.trim().toLowerCase()
        let liked = -1
        for (let i = 0; m && i < m.count; i++) {
            const e = m.get(i)
            if (e.kind === "liked")
                liked = e.trackCount ?? -1
        }
        likedTotal = liked
        for (let i = 0; m && i < m.count; i++) {
            const e = m.get(i)
            if (e.kind === "liked")
                continue
            if (q !== "" && ((e.title ?? "") + " " + (e.subtitle ?? "") + " " + (e.owner ?? "")).toLowerCase().indexOf(q) < 0)
                continue
            out.push(e)
        }
        entries = out
    }
    onModelChanged: { _rebuild(); _animateIn() }
    onQueryChanged: _rebuild()
    Component.onCompleted: _rebuild()
    Connections {
        target: page.model
        ignoreUnknownSignals: true
        function onCountChanged() { page._rebuild() }
        function onModelReset() { page._rebuild() }
        function onDataChanged() { page._rebuild() }
        function onRowsMoved() { page._rebuild() }
    }
    Connections {
        target: Library
        function onPinsChanged() { page._rebuild() }
    }

    // Tiles appearing after a section change: a cascade with a 25 ms delay (no more than 12 steps)
    property bool _cascade: false
    function _animateIn() {
        if (Theme.reducedMotion === true)
            return
        _cascade = true
        cascadeOff.restart()
    }
    Timer { id: cascadeOff; interval: 900; onTriggered: page._cascade = false }

    // ---- Grid: cell from 168, gap 28 x 22, rows by cols ----
    // The leftover width is divided among the tiles (they grow), rather than going into the gaps: otherwise with 2-3 columns
    // there would be gaping 100+ px gaps between the tiles
    readonly property real minCell: 168
    readonly property real gapY: 28
    readonly property real innerWidth: list.width - 64
    readonly property int cols: Math.max(2, Math.floor((innerWidth + 22) / (minCell + 22)))
    readonly property real cell: Math.max(minCell, Math.floor((innerWidth - (cols - 1) * 22) / cols))
    readonly property real gapX: cols > 1 ? Math.max(22, (innerWidth - cols * cell) / (cols - 1)) : 0
    readonly property var rows: {
        const out = []
        let row = []
        let used = 0
        if (showLiked) {
            row.push({ liked: true })
            used = 2
        }
        for (const e of entries) {
            if (used >= cols) {
                out.push(row)
                row = []
                used = 0
            }
            row.push(e)
            used++
        }
        if (row.length > 0)
            out.push(row)
        return out
    }
    readonly property var listEntries: showLiked ? [{ kind: "liked", title: I18n.t("Liked Songs"),
        subtitle: I18n.t("Playlist") + " · " + I18n.n(likedCount, "{0} track", "{0} tracks"), uri: "" }].concat(entries) : entries
    // the "Liked" total: from the kind "liked" row (the service knows the overall number), otherwise — how many are loaded
    property int likedTotal: -1
    readonly property int likedCount: likedTotal >= 0 ? likedTotal : Library.liked ? Library.liked.count : 0
    // changing the view or section — the list from the start
    onGridChanged: Qt.callLater(() => list.positionViewAtBeginning())
    onKindChanged: Qt.callLater(() => list.positionViewAtBeginning())

    function notify(text, actionText, onAction) {
        const w = page.Window.window
        if (w && typeof w.notify === "function")
            w.notify(text, actionText ?? "", onAction ?? null)
    }

    // ---- Actions ----
    // Hovering a card: the page is built in the background after a pause, a click opens it instantly.
    property var _prewarmedEntry: null
    function prewarmEntry(e) {
        if (!e || e.kind !== "playlist")
            return
        _prewarmedEntry = { name: "playlist", props: { playlistId: e.id, playlistTitle: e.title ?? "", cover: e.cover ?? "" } }
        router.prewarmHover(_prewarmedEntry.name, _prewarmedEntry.props)
    }
    function prewarmEntryCancel() {
        if (_prewarmedEntry)
            router.prewarmHoverCancel(_prewarmedEntry.name, _prewarmedEntry.props)
    }

    function open(e) {
        const k = e.kind ?? ""
        // A DJ is not a playlist: it has no page, the tile simply starts it
        if (k === "dj")
            play(e)
        else if (k === "liked")
            router.push("liked")
        else if (k === "playlist")
            router.push("playlist", { playlistId: e.id, playlistTitle: e.title ?? "", cover: e.cover ?? "" })
        else if (k === "album")
            Catalog.openAlbum(e.id)
        else if (k === "artist")
            Catalog.openArtist(e.id)
        else if (k === "show")
            Catalog.openShow(e.id)
    }
    function play(e) {
        if ((e.kind ?? "") === "liked" || e.liked) {
            if (Library.liked && Library.liked.count > 0)
                Player.playFrom(Library.liked, 0)
            return
        }
        if (e.uri)
            Player.playContext(e.uri, "")
    }
    function _isPlaying(e) {
        return Player.playing && (e.uri ?? "") !== "" && Player.contextUri === e.uri
    }

    property var _menuEntry: null
    function showMenu(e, point) {
        _menuEntry = e
        const p = tileMenu.mapFromItem(null, point.x, point.y)
        tileMenu.open(_menuItems(e), p)
    }
    function _menuItems(e) {
        const k = e.kind ?? ""
        if (k === "liked")
            return [
                { id: "play", text: I18n.t("Play"), icon: "play_arrow" },
                { id: "shuffle", text: I18n.t("Shuffle play"), icon: "shuffle" }
            ]
        const pinned = Library.pinsRevision >= 0 && Library.isPinned(e.uri ?? "")
        const radioText = ({ playlist: I18n.t("Playlist radio"), album: I18n.t("Album radio"), artist: I18n.t("Artist radio") })[k] ?? I18n.t("Radio")
        const items = [
            { id: "play", text: I18n.t("Play"), icon: "play_arrow" },
            { id: "shuffle", text: I18n.t("Shuffle play"), icon: "shuffle", enabled: !!Player.shuffleContext },
            { id: "radio", text: radioText, icon: "radio", enabled: k !== "show" },
            { separator: true },
            { id: "pin", text: pinned ? I18n.t("Unpin") : I18n.t("Pin to top"), icon: "push_pin", hint: I18n.t("local") }
        ]
        if (k === "playlist" && e.canEdit && !e.readOnly) {
            items.push({ id: "edit", text: I18n.t("Edit"), icon: "edit" })
            if ((e.ownerId ?? "") === (Auth.account.id ?? "-"))
                items.push({ id: "delete", text: I18n.t("Delete playlist"), icon: "delete" })
        } else if (k === "playlist") {
            items.push({ id: "unfollow", text: I18n.t("Unfollow"), icon: "remove_circle" })
        } else {
            items.push({ id: "unsave", text: k === "artist" ? I18n.t("Unfollow artist") : I18n.t("Remove from library"), icon: "remove_circle" })
        }
        items.push({ separator: true }, { id: "link", text: I18n.t("Copy link"), icon: "content_copy", enabled: (e.uri ?? "") !== "" })
        return items
    }
    function _menuAction(id) {
        const e = _menuEntry
        if (!e)
            return
        const t = e.title ?? ""
        if (id === "open" || id === "edit") {
            open(e)
        } else if (id === "play") {
            play(e)
        } else if (id === "shuffle") {
            if ((e.kind ?? "") === "liked")
                Player.shuffleFrom(Library.liked)
            else if (Player.shuffleContext)
                Player.shuffleContext(e.uri)
        } else if (id === "radio") {
            Radio.playStation(e.uri, t)
            notify(I18n.t("“{0}” radio", t))
        } else if (id === "pin") {
            const was = Library.isPinned(e.uri)
            Library.togglePin(e.uri)
            notify(was ? I18n.t("Unpinned") : I18n.t("Pinned — in this app only"))
        } else if (id === "delete" || id === "unfollow") {
            Library.unfollowPlaylist(e.id)
            notify(id === "delete" ? I18n.t("Playlist deleted") : I18n.t("Unfollowed “{0}”", t), I18n.t("Undo"), () => Library.followPlaylist(e.id))
        } else if (id === "unsave") {
            Library.toggleSaved(e.uri)
            notify(I18n.t("Removed from library"), I18n.t("Undo"), () => Library.toggleSaved(e.uri))
        } else if (id === "link") {
            Catalog.copyLink(Catalog.uriUrl(e.uri))
            notify(I18n.t("Link copied"))
        }
    }

    // ---- Top bar buttons: library search, sorting, grid | list ----
    readonly property Component toolbar: signedIn ? toolbarComponent : null
    Component {
        id: toolbarComponent
        Row {
            spacing: 8
            Item {
                width: page.searchOpen ? 240 : 0
                height: 40
                clip: true
                anchors.verticalCenter: parent.verticalCenter
                Behavior on width { enabled: Theme.reducedMotion !== true; NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
                SearchField {
                    id: libSearch
                    width: 240
                    height: 40
                    placeholder: I18n.t("Search in library")
                    text: page.query
                    onTextChanged: page.query = text
                    Keys.onEscapePressed: { page.query = ""; page.searchOpen = false }
                }
            }
            IconButton {
                anchors.verticalCenter: parent.verticalCenter
                icon: "search"
                checkable: true
                autoToggle: false
                checked: page.searchOpen
                onClicked: {
                    page.searchOpen = !page.searchOpen
                    if (page.searchOpen)
                        Qt.callLater(() => libSearch.inputItem.forceActiveFocus())
                    else
                        page.query = ""
                }
            }
            Button {
                id: sortButton
                visible: !page.inFolder
                anchors.verticalCenter: parent.verticalCenter
                style: "text"
                icon: "sort"
                text: page.width < 760 ? "" : page.sortText
                onClicked: {
                    const p = sortButton.mapToItem(sortMenu, 0, sortButton.height)
                    page._menuEntry = null
                    sortMenu.open(page.sorts.map(s => ({ id: s.id, text: s.text, checked: s.id === Settings.librarySort }))
                        .concat([{ separator: true }, { id: "info", text: I18n.t("Order isn't saved to YouTube Music"), icon: "info", enabled: false }]), p)
                }
            }
            ButtonGroup {
                anchors.verticalCenter: parent.verticalCenter
                model: [{ icon: "grid_view" }, { icon: "view_list" }]
                autoSelect: false
                currentIndex: page.grid ? 0 : 1
                onActivated: i => page._switchView(i === 0 ? "grid" : "list")
            }
        }
    }

    // Grid <-> list: fades out and appears anew
    function _switchView(v) {
        if (Settings.libraryView === v)
            return
        if (Theme.reducedMotion === true) {
            Settings.libraryView = v
            return
        }
        viewSwap.next = v
        viewSwap.restart()
    }
    SequentialAnimation {
        id: viewSwap
        property string next
        NumberAnimation { target: list; property: "opacity"; to: 0; duration: Theme.motion.effectsDefault / 2 }
        ScriptAction { script: { Settings.libraryView = viewSwap.next; page._animateIn() } }
        NumberAnimation { target: list; property: "opacity"; to: 1; duration: Theme.motion.effectsDefault }
    }

    ListView {
        id: list
        anchors.fill: parent
        model: !page.signedIn ? [] : page.grid ? page.rows : page.listEntries
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        acceptedButtons: Qt.NoButton
        cacheBuffer: 800
        // the delegate is a Loader with a stable sourceComponent (it changes only when grid/list switches),
        // so rows can be reused when scrolling
        reuseItems: true
        spacing: page.grid ? page.gapY : 0

        header: Item {
            width: list.width
            height: page.signedIn ? headerCol.implicitHeight + (page.grid ? 20 : 12) : 0
            visible: page.signedIn

            ColumnLayout {
                id: headerCol
                x: 32
                y: 4
                width: parent.width - 64
                spacing: 12

                // Sections
                Flow {
                    visible: true
                    Layout.fillWidth: true
                    spacing: 8
                    Repeater {
                        model: [
                            { id: "playlists", text: I18n.t("Playlists"), n: Library.playlists.count },
                            { id: "albums", text: I18n.t("Albums"), n: Library.albums.count },
                            { id: "artists", text: I18n.t("Artists"), n: Library.artists.count },
                            { id: "shows", text: I18n.t("Podcasts"), n: Library.shows.count }
                        ]
                        Chip {
                            required property var modelData
                            text: modelData.text + (modelData.n > 0 ? "  " + modelData.n : "")
                            selected: page.kind === modelData.id
                            onClicked: { page.kind = modelData.id; page.owner = "all" }
                        }
                    }
                }
                // Owner (playlists only)
                Flow {
                    visible: page.kind === "playlists"
                    Layout.fillWidth: true
                    spacing: 8
                    Repeater {
                        model: [
                            { id: "all", text: I18n.t("All") }, { id: "mine", text: I18n.t("Mine") },
                            { id: "followed", text: I18n.t("Followed") }, { id: "collab", text: I18n.t("Collaborative") }
                        ]
                        Chip {
                            required property var modelData
                            implicitHeight: 28
                            text: modelData.text
                            selected: page.owner === modelData.id
                            onClicked: page.owner = modelData.id
                        }
                    }
                }

                // Explicit sorting loads more pages
                Rectangle {
                    visible: Library.pagesTotal > 0 && Library.pagesLoaded < Library.pagesTotal
                    implicitWidth: loadRow.implicitWidth + 18
                    implicitHeight: 32
                    radius: 16
                    color: Theme.secondaryContainer
                    Row {
                        id: loadRow
                        anchors.verticalCenter: parent.verticalCenter
                        x: 6
                        spacing: 8
                        LoadingIndicator { size: 22; color: Theme.fgSecondaryContainer; running: parent.parent.visible; anchors.verticalCenter: parent.verticalCenter }
                        Label {
                            anchors.verticalCenter: parent.verticalCenter
                            text: I18n.t("Loading remaining pages · {0} of {1} — loaded items are already here", Library.pagesLoaded, Library.pagesTotal)
                            type: "labelMedium"
                            color: Theme.fgSecondaryContainer
                        }
                    }
                }

                // Tooltips
                Rectangle {
                    visible: page.kind === "shows"
                    Layout.fillWidth: true
                    implicitHeight: hintRow.implicitHeight + 24
                    radius: Theme.shape.large
                    color: Theme.surfaceContainerHigh
                    RowLayout {
                        id: hintRow
                        x: 16
                        y: 12
                        width: parent.width - 32
                        spacing: 12
                        Icon { Layout.alignment: Qt.AlignTop; name: "info"; size: 20; color: Theme.primary }
                        Label {
                            Layout.fillWidth: true
                            text: I18n.t("Audiobooks from saved shows are hidden: librespot can't play them.")
                            type: "bodySmall"
                            color: Theme.fgSurfaceVariant
                            wrapMode: Text.WordWrap
                        }
                    }
                }

                // List column headers
                RowLayout {
                    visible: !page.grid && page.listEntries.length > 0
                    Layout.fillWidth: true
                    Layout.preferredHeight: 36
                    spacing: 16
                    readonly property string sort: Settings.librarySort
                    Head { Layout.fillWidth: true; Layout.preferredWidth: 3; text: I18n.t("Title") + (active ? "  ↓" : ""); active: parent.sort === "alpha" }
                    Head { Layout.fillWidth: true; Layout.preferredWidth: 1.4; text: I18n.t("Creator") + (active ? "  ↓" : ""); active: parent.sort === "author" }
                    Head { visible: list.width - 40 >= 640; Layout.preferredWidth: 120; text: I18n.t("Added") + (active ? "  ↓" : ""); active: parent.sort === "added" }
                    Head { visible: list.width - 40 >= 860; Layout.preferredWidth: 120; text: I18n.t("Last played") + (active ? "  ↓" : ""); active: parent.sort === "recent" }
                    Item { Layout.preferredWidth: 32 }
                }
                Rectangle {
                    visible: !page.grid && page.listEntries.length > 0
                    Layout.fillWidth: true
                    Layout.topMargin: -12
                    height: 1
                    color: Theme.outlineVariant
                }
            }
        }

        delegate: Loader {
            id: cellLoader
            required property var modelData
            required property int index
            width: list.width
            sourceComponent: page.grid ? gridRow : listRow
        }

        footer: Item { width: list.width; height: 140 }
    }
    SmoothScroll { flickable: list; wheelStep: Settings.wheelStep }

    // Grid row: tiles by cols, evenly across the width
    Component {
        id: gridRow
        Item {
            id: rowItem
            readonly property var items: parent ? parent.modelData : []
            readonly property int rowIndex: parent ? parent.index : 0
            width: list.width
            height: rowContent.implicitHeight
            Row {
                id: rowContent
                x: 32
                spacing: page.gapX
                Repeater {
                    model: rowItem.items
                    Loader {
                        id: tileLoader
                        required property var modelData
                        required property int index
                        readonly property int order: rowItem.rowIndex * page.cols + index
                        sourceComponent: modelData.liked ? likedTile : mediaTile
                        // appearance cascade: fade + 12 px offset from below
                        opacity: 1
                        transform: Translate { id: shift; y: 0 }
                        Component.onCompleted: {
                            if (!page._cascade)
                                return
                            opacity = 0
                            shift.y = 12
                            appear.start()
                        }
                        SequentialAnimation {
                            id: appear
                            PauseAnimation { duration: Math.min(tileLoader.order, 12) * 25 }
                            ParallelAnimation {
                                NumberAnimation { target: tileLoader; property: "opacity"; to: 1; duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedDecelerate }
                                NumberAnimation { target: shift; property: "y"; to: 0; duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedDecelerate }
                            }
                        }
                        Component {
                            id: likedTile
                            LikedTile {
                                width: 2 * page.cell + page.gapX
                                trackCount: page.likedCount
                                height: page.cell + 48
                                onClicked: page.open({ kind: "liked" })
                                onPlayClicked: page.play({ kind: "liked" })
                                onMenuRequested: p => page.showMenu({ kind: "liked" }, p)
                            }
                        }
                        Component {
                            id: mediaTile
                            MediaTile {
                                readonly property var e: tileLoader.modelData
                                size: page.cell
                                cover: e.cover ?? ""
                                title: e.title ?? ""
                                subtitle: e.subtitle ?? ""
                                round: e.kind === "artist"
                                placeholderIcon: ({ playlist: "queue_music", album: "album", artist: "person", show: "podcasts" })[e.kind] ?? "album"
                                pinned: e.pinned === true
                                showPlay: true
                                playing: page._isPlaying(e)
                                onClicked: page.open(e)
                                onPlayClicked: page._isPlaying(e) ? Player.togglePlay() : page.play(e)
                                onMenuRequested: p => page.showMenu(e, p)
                                onHoveredChanged: hovered ? page.prewarmEntry(e) : page.prewarmEntryCancel()
                            }
                        }
                    }
                }
            }
        }
    }

    Component {
        id: listRow
        Item {
            readonly property var e: parent ? parent.modelData : ({})
            implicitHeight: 64
            LibraryRow {
                x: 20
                width: parent.width - 40
                entry: parent.e
                onClicked: page.open(parent.e)
                onMenuRequested: p => page.showMenu(parent.e, p)
            }
        }
    }

    // ---- Loading, empty, not signed in ----
    Grid {
        visible: page.signedIn && page.model !== null && page.model.loading === true && page.model.count === 0
        x: 32
        y: 120
        width: page.width - 64
        columns: page.cols
        columnSpacing: page.gapX
        rowSpacing: page.gapY
        Repeater {
            model: 10
            SkeletonTile { size: page.cell }
        }
    }
    EmptyState {
        anchors.horizontalCenter: parent.horizontalCenter
        y: Math.max(140, (parent.height - implicitHeight) / 2)
        width: Math.min(parent.width - 64, 420)
        visible: page.signedIn && page.model !== null && page.model.loading !== true && page.entries.length === 0 && !page.showLiked
        // failed to load (the session did not answer, the Web API is rate-limited at 429) — an error and "Retry", not "empty"
        readonly property bool failed: Library.errorText !== "" && page.query.trim() === "" && page.owner === "all"
        readonly property var texts: failed
            ? ["cloud_off", I18n.t("Couldn’t load your library"), Library.errorText]
            : page.query.trim() !== "" || page.owner !== "all"
            ? ["search_off", I18n.t("Nothing found"), page.query.trim() !== "" ? I18n.t("Try a different search") : I18n.t("This section is empty")]
            : ({
                playlists: ["queue_music", I18n.t("No playlists"), I18n.t("Playlists you create or save will appear here.")],
                albums: ["album", I18n.t("No saved albums"), I18n.t("Saved albums will appear here.")],
                artists: ["person", I18n.t("No followed artists"), I18n.t("Artists you follow will appear here.")],
                shows: ["podcasts", I18n.t("No podcasts yet"), I18n.t("Podcasts you follow will appear here.")]
            })[page.kind]
        icon: texts[0]
        title: texts[1]
        text: texts[2]
        // nothing was found — the button removes whatever narrowed the list (search or the owner filter)
        actionText: failed ? I18n.t("Retry")
                  : page.query.trim() !== "" ? I18n.t("Clear search")
                  : page.owner !== "all" ? I18n.t("Show all")
                  : page.kind === "albums" ? I18n.t("Find albums") : ""
        actionIcon: failed ? "refresh" : page.query.trim() !== "" || page.owner !== "all" ? "close" : "search"
        onAction: {
            if (failed) {
                Library.refresh()
            } else if (page.query.trim() !== "") {
                page.query = ""
                page.searchOpen = false
            } else if (page.owner !== "all") {
                page.owner = "all"
            } else {
                page.router.reset("search")
            }
        }
    }
    EmptyState {
        anchors.centerIn: parent
        width: Math.min(parent.width - 64, 420)
        visible: !page.signedIn
        icon: "library_music"
        title: I18n.t("Sign in to see your library")
        text: I18n.t("Playlists, albums, artists and podcasts will load from your YouTube Music account")
        actionText: I18n.t("Sign in")
        actionIcon: "login"
        onAction: page.router.reset("account")
    }

    Menu {
        id: tileMenu
        anchors.fill: parent
        z: 50
        onTriggered: (id, item) => page._menuAction(id)
    }
    Menu {
        id: sortMenu
        anchors.fill: parent
        z: 50
        onTriggered: (id, item) => { if (id !== "info") Settings.librarySort = id }
    }
}
