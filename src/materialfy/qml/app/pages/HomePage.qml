import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core
import ".."

// Home: the "Radio" scene at the top (station seeds on the orbit), below — the Home.shelves shelves: "Quick access",
// "Recently played", "Made for you", "Continue podcasts", "Top artists", "Top tracks".
Page {
    id: page

    title: I18n.t("Home")
    showHeader: false

    readonly property bool signedIn: Auth.state === "signedIn"
    readonly property bool compact: width < 760
    // Shelves: a Repeater over the shelf composition [{key, kind}] — it changes only if shelves were added, removed
    // or reordered. A shelf data goes into its Loader in place and only if changed: updating
    // "Recently played" does not recreate "Made for you" and the other tiles.
    property var shelfIds: []
    function _syncShelves() {
        // The YouTube Music home page also works without sign-in (guest shelves: picks, new releases) —
        // so the shelves are always shown as soon as the service provides them.
        const list = Home.shelves ?? []
        const ids = list.map(s => ({ key: s.key ?? "", kind: s.kind ?? "" }))
        if (JSON.stringify(ids) !== JSON.stringify(shelfIds))
            shelfIds = ids
        for (let i = 0; i < list.length; ++i) {
            const loader = shelfRepeater.itemAt(i)
            if (!loader)
                continue
            const json = JSON.stringify(list[i])
            if (loader.json !== json) {
                loader.json = json
                loader.shelfData = list[i]
            }
        }
    }
    Connections {
        target: Home
        function onShelvesChanged() { page._syncShelves() }
    }
    onSignedInChanged: _syncShelves()

    onVisibleChanged: if (visible) Home.refresh(false)
    Component.onCompleted: {
        _syncShelves()
        Home.refresh(false)
    }
    Connections {
        target: Auth
        function onSignedIn() { Home.refresh(false) }
    }

    function notify(text, actionText, onAction) {
        const w = page.Window.window
        if (w && typeof w.notify === "function")
            w.notify(text, actionText ?? "", onAction ?? null)
    }
    // Scroll to the radio scene (after "Radio by track" from the menu)
    function scrollToTop() { flickable.contentY = 0 }

    // ---- Tiles: open / play / menu ----
    function openEntry(e) {
        const k = e.kind ?? ""
        if (k === "dj")             // a DJ has no track list: the tile starts it right away
            playEntry(e)
        else if (k === "liked")
            router.push("liked")
        else if (k === "playlist")
            router.push("playlist", { playlistId: e.id, playlistTitle: e.title ?? "", cover: e.cover ?? "" })
        else if (k === "album")
            router.push("album", { albumId: e.id, cover: e.cover ?? "" })
        else if (k === "artist")
            router.push("artist", { artistId: e.id, cover: e.cover ?? "" })
        else if (k === "show")
            router.push("show", { showId: e.id, cover: e.cover ?? "" })
        else if (k === "episode")
            router.push("episode", { episodeId: e.id })
        else if (k === "radio")     // id — the seed (spotify:track:...), uri — the station; Radio.open understands both
            router.push("radio", { seed: (e.id ?? "") !== "" ? e.id : (e.uri ?? "") })
        else if (e.uri)
            Catalog.openUri(e.uri)
    }
    // is playing (or paused) exactly from here
    function isCurrent(e) {
        return Player.hasTrack && ((e.kind ?? "") === "liked" ? Player.source === "liked"
                                   : (e.uri ?? "") !== "" && Player.contextUri === e.uri)
    }
    // toggle — the button on the tile: for something playing from here it shows pause and pauses, rather than starts
    // again (for radio — without a new selection of 50 tracks); "Listen" in the menu only resumes
    function playEntry(e, toggle) {
        const k = e.kind ?? ""
        if (isCurrent(e)) {
            if (toggle !== false || !Player.playing)
                Player.togglePlay()
            return
        }
        if (k === "liked") {
            if (Library.liked && Library.liked.count > 0)
                Player.playFrom(Library.liked, 0)
            else if (e.uri)
                Player.playContext(e.uri, "")
        } else if (k === "radio") {
            Radio.playStation(e.uri, e.title ?? "")
        } else if (e.uri) {
            Player.playContext(e.uri, "")
        }
    }
    function isPlaying(e) {
        return Player.playing && (e.uri ?? "") !== "" && Player.contextUri === e.uri
    }
    property var _menuEntry: null
    function showMenu(e, point) {
        _menuEntry = e
        const k = e.kind ?? ""
        const items = [{ id: "play", text: I18n.t("Play"), icon: "play_arrow" }]
        if (k !== "radio" && k !== "artist" && k !== "dj")
            items.push({ id: "shuffle", text: I18n.t("Shuffle play"), icon: "shuffle", enabled: k === "liked" || !!Player.shuffleContext })
        if (k === "playlist" || k === "album" || k === "artist")
            items.push({ id: "radio", text: ({ playlist: I18n.t("Playlist radio"), album: I18n.t("Album radio"), artist: I18n.t("Artist radio") })[k], icon: "radio",
                         enabled: !Radio.locked })
        if (k !== "liked" && k !== "radio" && k !== "dj" && (e.uri ?? "") !== "") {
            const pinned = Library.pinsRevision >= 0 && Library.isPinned(e.uri)
            items.push({ separator: true }, { id: "pin", text: pinned ? I18n.t("Unpin") : I18n.t("Pin in library"), icon: "push_pin", hint: I18n.t("local") },
                       { id: "link", text: I18n.t("Copy link"), icon: "content_copy" })
        }
        tileMenu.open(items, tileMenu.mapFromItem(null, point.x, point.y))
    }
    function _menuAction(id) {
        const e = _menuEntry
        if (!e)
            return
        if (id === "play")
            playEntry(e, false)
        else if (id === "shuffle")
            e.kind === "liked" ? Player.shuffleFrom(Library.liked) : Player.shuffleContext(e.uri)
        else if (id === "radio") {
            Radio.playStation(e.uri, e.title ?? "")
            notify(I18n.t("“{0}” radio", e.title ?? ""))
        } else if (id === "pin") {
            const was = Library.isPinned(e.uri)
            Library.togglePin(e.uri)
            notify(was ? I18n.t("Unpinned") : I18n.t("Pinned — in this app only"))
        } else if (id === "link") {
            Catalog.copyLink(Catalog.uriUrl(e.uri))
            notify(I18n.t("Link copied"))
        }
    }

    // ---- "Radio" scene ----
    RadioOrbit {
        id: orbit
        // the "Radio" scene is visible by default; it can be hidden in settings — the "Radio" page and
        // "Radio by track" remain
        visible: Settings.homeRadioHidden !== true
        Layout.fillWidth: true
        Layout.topMargin: 8
        Layout.preferredHeight: implicitHeight
        compact: page.compact
        signedIn: page.signedIn
        premium: Auth.premium
        groups: page.signedIn ? Radio.seeds : []
        selected: Radio.selected
        locked: page.signedIn && Radio.locked
        loading: Radio.loading
        playing: Radio.active && Player.playing
        errorText: Radio.errorText
        onPicked: uri => Radio.select(uri)
        onAddRequested: item => finder.open(item)
        onPlayClicked: Radio.play()
        onOpenList: page.router.push("radio", { seed: Radio.selected })
        onSaveRequested: {
            Radio.save()
            page.notify(I18n.t("Saving station as a playlist…"))
        }
        onRefreshRequested: Radio.refresh()
        onConnectRequested: page.router.reset(Auth.premium ? "playback" : "account")
        onMoreRequested: page.router.reset("settings", { section: "playback" })
        onLoginRequested: page.router.reset("account")
    }

    // ---- Shelves ----
    ColumnLayout {
        Layout.fillWidth: true
        visible: true
        spacing: 12


        // Loading: placeholders without flicker
        ColumnLayout {
            visible: Home.loading && page.shelfIds.length === 0
            Layout.fillWidth: true
            spacing: 12
            SectionHeader { text: I18n.t("Quick access") }
            GridLayout {
                Layout.fillWidth: true
                columns: page.compact ? 2 : 3
                columnSpacing: 10
                rowSpacing: 10
                Repeater {
                    model: 6
                    Rectangle { Layout.fillWidth: true; Layout.preferredHeight: 64; radius: Theme.shape.large; color: Theme.surfaceContainerHigh }
                }
            }
            SectionHeader { text: I18n.t("Recently played") }
            // the placeholder shelf is wider than the page: it is clipped, rather than stretching the column (and the scene above it)
            Item {
                Layout.fillWidth: true
                Layout.preferredHeight: skelRow.implicitHeight
                clip: true
                Row {
                    id: skelRow
                    spacing: 24
                    Repeater { model: 6; SkeletonTile { size: 168 } }
                }
            }
        }

        EmptyState {
            visible: Home.errorText !== "" && page.shelfIds.length === 0 && !Home.loading
            Layout.alignment: Qt.AlignHCenter
            Layout.topMargin: 24
            icon: "cloud_off"
            title: I18n.t("Couldn't load shelves")
            text: Home.errorText
            actionText: I18n.t("Retry")
            actionIcon: "refresh"
            onAction: Home.refresh(true)
        }

        Repeater {
            id: shelfRepeater
            model: page.shelfIds
            Loader {
                id: shelf
                required property var modelData        // {key, kind}
                property var shelfData: ({})
                property string json
                Layout.fillWidth: true
                sourceComponent: modelData.kind === "tracks" ? tracksShelf
                    : rowShelf
            }
        }
    }

    // Top tracks: two columns of rows (one in a narrow window), "Show more"
    Component {
        id: tracksShelf
        ColumnLayout {
            id: tracksCol
            readonly property var shelfData: parent ? parent.shelfData : ({})
            readonly property var tracks: shelfData.items ?? []
            property bool expanded: false
            spacing: 12
            SectionHeader {
                text: tracksCol.shelfData.title ?? ""
                actionText: tracksCol.tracks.length > 8 ? (tracksCol.expanded ? I18n.t("Show less") : I18n.t("Show more")) : ""
                onAction: tracksCol.expanded = !tracksCol.expanded
            }
            GridLayout {
                Layout.fillWidth: true
                Layout.leftMargin: -12
                Layout.rightMargin: -12
                columns: page.compact ? 1 : 2
                columnSpacing: 16
                rowSpacing: 0
                Repeater {
                    model: tracksCol.expanded ? tracksCol.tracks : tracksCol.tracks.slice(0, 8)
                    TrackRow {
                        required property var modelData
                        required property int index
                        Layout.fillWidth: true
                        number: index + 1
                        showAlbum: false
                        track: modelData
                        trackId: modelData.trackId ?? ""
                        title: modelData.title ?? ""
                        version: modelData.version ?? ""
                        artists: modelData.artists ?? ""
                        artistRefs: modelData.artistRefs ?? []
                        albumId: modelData.albumId ?? ""
                        album: modelData.album ?? ""
                        cover: modelData.cover ?? ""
                        uri: modelData.uri ?? ""
                        durationMs: modelData.durationMs ?? 0
                        explicit: modelData.explicit ?? false
                        available: modelData.available ?? true
                        liked: Library.likesRevision >= 0 && Library.isLiked(trackId)
                        current: trackId !== "" && trackId === Player.trackId
                        onActivated: {
                            // the home page row comes from the Web API, not the catalog: without it the screen would wait for sound
                            Player.rememberTrack(modelData)
                            Player.playUris(tracksCol.tracks.map(t => t.uri), index)
                        }
                        onLikeClicked: Library.toggleLike(modelData)
                    }
                }
            }
        }
    }

    // Horizontal shelf (tiles, episodes, artists): the arrows page through it, the wheel scrolls the page
    Component {
        id: rowShelf
        ColumnLayout {
            id: rowCol
            readonly property var shelfData: parent ? parent.shelfData : ({})
            readonly property string kind: shelfData.kind ?? "tiles"
            // Is the shelf wider than the page? The condition for the paging arrows (see below why not via visible)
            readonly property bool scrollable: strip.contentWidth > strip.width + 1
            spacing: 12
            RowLayout {
                Layout.fillWidth: true
                Layout.topMargin: 8
                spacing: 8
                z: 1        // the info tooltip is longer than the title line: over the shelf tiles, not under them
                Label {
                    Layout.fillWidth: true
                    Layout.maximumWidth: Math.ceil(implicitWidth)
                    text: rowCol.shelfData.title ?? ""
                    type: "headlineSmall"
                    weight: 600
                }
                Item { Layout.fillWidth: true }
                // The arrows are not hidden via visible: showing/hiding changes the row width, on which
                // strip.width depends, and the "content is wider than the shelf" condition would start flickering — ColumnLayout
                // would re-polish endlessly (in the shelf layout, "possible polish() loop", the cards
                // would overlap each other). The space for the arrows is always reserved, and the invisible ones are disabled.
                IconButton {
                    icon: "chevron_left"
                    size: "xs"
                    enabled: rowCol.scrollable && !strip.atXBeginning
                    opacity: rowCol.scrollable ? 1 : 0
                    onClicked: strip.pageBy(-1)
                }
                IconButton {
                    icon: "chevron_right"
                    size: "xs"
                    enabled: rowCol.scrollable && !strip.atXEnd
                    opacity: rowCol.scrollable ? 1 : 0
                    onClicked: strip.pageBy(1)
                }
            }
            ListView {
                id: strip
                Layout.fillWidth: true
                Layout.leftMargin: -32
                Layout.rightMargin: -32
                Layout.preferredHeight: rowCol.kind === "episode" ? 104 : rowCol.kind === "artist" ? 200 : 220
                orientation: ListView.Horizontal
                spacing: 24
                leftMargin: 32
                rightMargin: 32
                interactive: false
                clip: true
                // tiles beyond the edge are not created, the ones that went beyond the edge are reused
                reuseItems: true
                cacheBuffer: 360
                model: rowCol.shelfData.items ?? []
                function pageBy(dir) {
                    const target = Math.max(originX - leftMargin, Math.min(originX + contentWidth - width + rightMargin,
                                            contentX + dir * (width - 120)))
                    scrollAnim.to = target
                    scrollAnim.restart()
                }
                NumberAnimation {
                    id: scrollAnim
                    target: strip
                    property: "contentX"
                    duration: Theme.reducedMotion === true ? 0 : Theme.motion.spatialDefault
                    easing.type: Easing.BezierSpline
                    easing.bezierCurve: Theme.motion.emphasized
                }
                // Shift + wheel — page the shelf
                WheelHandler {
                    acceptedModifiers: Qt.ShiftModifier
                    onWheel: event => {
                        const d = event.angleDelta.y !== 0 ? event.angleDelta.y : event.angleDelta.x
                        strip.contentX = Math.max(strip.originX - strip.leftMargin,
                            Math.min(strip.originX + strip.contentWidth - strip.width + strip.rightMargin, strip.contentX - d))
                    }
                }
                delegate: Loader {
                    required property var modelData
                    sourceComponent: rowCol.kind === "episode" ? episodeCard : rowCol.kind === "artist" ? artistTile : mediaTile
                }
                // Tiles — one component per shelf, not three per tile
                Component {
                    id: mediaTile
                    MediaTile {
                        readonly property var e: parent ? parent.modelData : ({})
                        size: 168
                        cover: e.cover ?? ""
                        coverKind: e.kind === "album" || e.kind === "artist" ? e.kind : ""
                        coverArtist: e.kind === "artist" ? (e.title ?? "") : (e.subtitle ?? "").split(", ")[0]
                        coverTitle: e.kind === "album" ? (e.title ?? "") : ""
                        title: e.title ?? ""
                        subtitle: e.subtitle ?? ""
                        round: e.kind === "artist"
                        shape: e.kind === "artist" ? "circle" : "softSquare"
                        badge: e.kind === "radio" ? I18n.t("RADIO") : ""
                        placeholderIcon: ({ playlist: "queue_music", album: "album", artist: "person", show: "podcasts", radio: "radio", dj: "graphic_eq" })[e.kind] ?? "album"
                        showPlay: true
                        playing: page.isPlaying(e)
                        onClicked: page.openEntry(e)
                        onPlayClicked: page.playEntry(e)
                        onMenuRequested: p => page.showMenu(e, p)
                        onHoveredChanged: {
                            if (e.kind !== "playlist")
                                return
                            const props = { playlistId: e.id, playlistTitle: e.title ?? "", cover: e.cover ?? "" }
                            hovered ? router.prewarmHover("playlist", props)
                                    : router.prewarmHoverCancel("playlist", props)
                        }
                    }
                }
                Component {
                    id: artistTile
                    MediaTile {
                        readonly property var a: parent ? parent.modelData : ({})
                        size: 148
                        round: true
                        cover: a.cover ?? ""
                        coverKind: "artist"
                        coverArtist: a.name ?? ""
                        title: a.name ?? ""
                        subtitle: I18n.t("Artist")
                        placeholderIcon: "person"
                        onClicked: Catalog.openArtist(a.artistId ?? "")
                        onMenuRequested: p => page.showMenu({ kind: "artist", uri: a.uri ?? "", title: a.name ?? "", id: a.artistId ?? "" }, p)
                    }
                }
                Component {
                    id: episodeCard
                    EpisodeCard {
                        readonly property var ep: parent ? parent.modelData : ({})
                        episode: ep
                        onClicked: Catalog.openEpisode(ep.episodeId ?? "")
                    }
                }
            }
        }
    }

    RadioSeedFinder {
        id: finder
        parent: page
        anchors.fill: parent
        z: 40
        onPicked: (uri, title) => Radio.select(uri)
    }
    Menu {
        id: tileMenu
        parent: page
        anchors.fill: parent
        z: 50
        onTriggered: (id, item) => page._menuAction(id)
    }
}
