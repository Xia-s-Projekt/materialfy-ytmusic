import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Page with a track list (virtualized ListView): the header scrolls along with the list.
// Search within the list (button in the header or Ctrl+F): while the field has text, the list shows the matches,
// and "Play", "Shuffle", clicking a track queue exactly what was found.
// editing — editing your own playlist: rows with a handle and "remove", reordering (moveRequested),
// the header is editHero instead of the usual one.
Item {
    id: page

    property var router
    property string title
    property string subtitle
    property string description
    property bool showHeader: true
    property var model: null                  // DictListModel of tracks
    property string emptyIcon: "music_note"
    property string emptyTitle: I18n.t("Nothing here")
    property string emptyText: ""
    property string emptyActionText: ""       // button under an empty list (for example, "Repeat")
    property string emptyActionIcon: ""
    signal emptyAction()
    property Component headerExtra: null       // extra header content (buttons etc.)
    property Component heroMeta: null          // line under the subtitle (owner, badges)
    property Component heroNote: null          // hint under the buttons
    property Component footerExtra: null       // under the list (for example, "+ Add tracks")
    // "Hero" header: cover + overline + title + "N tracks · duration" + Play/Shuffle
    property string overline                   // "PLAYLIST", "COLLECTION"
    property string heroImage                  // cover (url, coverLarge is better); if empty — a shape with an icon
    property string heroIcon: "queue_music"
    property var heroShape: "softSquare"
    property var heroArtistRefs: []            // artist links under the title (album)
    property bool showPlayButtons: true
    readonly property bool scrolled: list.contentY - list.originY > 48

    // Playlist editing
    property bool editing: false
    property Component editHero: null
    property string addedColumn                // "" / "by" / "at" — the "Added by" / "Added" column
    property bool showPlays: false             // play count column (album)
    property bool showAlbum: true              // album column (not needed on the album page)
    property real contentRightInset: 0         // space on the right for the side panel (AddTracksPanel)
    Behavior on contentRightInset { enabled: Theme.reducedMotion !== true; NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedDecelerate } }
    readonly property alias listView: list

    property bool searchOpen: false
    property string searchText
    // the query goes to the filter with a delay: a full model reset per character is expensive
    onSearchTextChanged: searchDebounce.restart()
    readonly property var shownModel: model ? filter : null   // what is in the list: everything or the matches

    signal trackActivated(int row)
    signal trackRemoved(int row)               // "remove" in edit mode (row — index in model)
    signal moveRequested(int from, int to)

    onEditingChanged: if (editing) closeSearch()

    // Scroll to a track and flash it once the list has loaded (once)
    property string highlightTrackId
    property string _flashId
    property int _flashRow: -1
    function _revealTrack() {
        if (highlightTrackId === "" || !shownModel || shownModel.count === 0)
            return
        for (let i = 0; i < shownModel.count; ++i) {
            if (shownModel.get(i).trackId === highlightTrackId) {
                list.userScrolled = true
                list.positionViewAtIndex(i, ListView.Center)
                _flashId = highlightTrackId
                highlightTrackId = ""
                flashReset.restart()
                return
            }
        }
    }
    // Flash a row by index (after adding to the end / restoring a deleted one)
    function flashRow(row) {
        if (row < 0)
            return
        _flashRow = row
        list.userScrolled = true
        list.positionViewAtIndex(row, ListView.Contain)
        flashReset.restart()
    }
    onHighlightTrackIdChanged: Qt.callLater(_revealTrack)
    Connections {
        target: page.shownModel
        ignoreUnknownSignals: true
        function onCountChanged() { Qt.callLater(page._revealTrack) }
        function onModelReset() { Qt.callLater(page._revealTrack) }
    }
    Timer { id: flashReset; interval: Theme.reducedMotion === true ? 1000 : 1600; onTriggered: { page._flashId = ""; page._flashRow = -1 } }

    // Ctrl+F: open search within the list. false — nothing to search here or the field is already focused
    // (then a repeated Ctrl+F goes to the global search).
    function findInPage() {
        if (!model || model.count === 0 || !showHeader || editing)
            return false
        const field = list.headerItem ? list.headerItem.searchField : null
        if (searchOpen && field && field.inputItem.activeFocus)
            return false
        searchOpen = true
        list.userScrolled = false
        list.positionViewAtBeginning()
        Qt.callLater(() => list.headerItem && list.headerItem.searchField && list.headerItem.searchField.inputItem.forceActiveFocus())
        return true
    }
    function closeSearch() {
        searchOpen = false
        searchText = ""
    }

    TrackFilter {
        id: filter
        source: page.model
        // query goes through searchDebounce: binding to page.searchText reset the model on every character
    }
    Timer {
        id: searchDebounce
        interval: 200
        onTriggered: filter.query = page.searchText
    }

    function _duration(ms) {
        const m = Math.round(ms / 60000)
        return m >= 60 ? I18n.t("{0} h {1} min", Math.floor(m / 60), m % 60) : I18n.t("{0} min", m)
    }

    // Slowly drifting shapes behind the header. The layer sits outside the ListView (otherwise its clip cuts the shapes at the
    // edge of the list) and slides under the transparent top panel; top and bottom the shapes dissolve,
    // so there is no visible border. It travels with the header, freezes when it has scrolled out of view.
    // The header's decorative motion starts once the page has arrived and the list has settled: opening is already
    // the heaviest moment (creating the page and the rows, the transition), so we add no repaints to it
    property bool _settled: false
    Timer { interval: 900; running: page.visible && !page._settled; onTriggered: page._settled = true }

    HeroShapes {
        width: page.width - page.contentRightInset
        active: page.showHeader && page.visible && Theme.reducedMotion !== true
        running: page._settled
        headerHeight: list.headerItem ? list.headerItem.height : 320
        scroll: list.contentY - list.originY
        contentX: list.x
        contentWidth: list.width
        seed: page.title
    }

    ReorderListView {
        id: list
        anchors.fill: parent
        anchors.leftMargin: 16
        anchors.rightMargin: 16 + page.contentRightInset
        model: page.shownModel
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        acceptedButtons: Qt.NoButton
        reuseItems: true
        cacheBuffer: 640
        reorderable: page.editing && !filter.active
        interactive: !dragActive
        onMoveRequested: (from, to) => page.moveRequested(from, to)

        // removal in edit mode: the row fades out, the neighbours pull up
        remove: Transition {
            enabled: page.editing && Theme.reducedMotion !== true
            NumberAnimation { property: "opacity"; to: 0; duration: Theme.motion.spatialFast; easing.type: Easing.OutCubic }
        }
        removeDisplaced: Transition {
            enabled: page.editing && Theme.reducedMotion !== true
            NumberAnimation { property: "y"; duration: Theme.motion.spatialFast; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized }
        }

        // ListView does not shift contentY when the header changes height (the "N tracks" subtitle
        // appears after loading) or the model is reset, — and the heading ends up cut off.
        // Until the user has scrolled, keep the list at the very top.
        property bool userScrolled: false
        function keepTop() { if (!userScrolled) Qt.callLater(() => contentY = originY) }
        onMovementStarted: userScrolled = true   // touch
        Connections {
            target: page.shownModel
            ignoreUnknownSignals: true
            function onModelReset() { if (!list._settling && list.dragFrom < 0) { list.userScrolled = false; list.keepTop() } }
        }

        header: Item {
            id: headerItem
            readonly property var searchField: heroLoader.item ? heroLoader.item.searchField ?? null : null
            width: list.width
            implicitHeight: (page.showHeader ? heroLoader.implicitHeight : 0) + 56
            onImplicitHeightChanged: list.keepTop()

            Loader {
                id: heroLoader
                visible: page.showHeader
                x: 16
                y: 24     // top margin: the shape behind the cover is larger than it and must not press against the list edge
                width: parent.width - 32
                sourceComponent: page.editing && page.editHero ? page.editHero : defaultHero
            }
        }

        delegate: TrackRow {
            id: row
            required property int index
            required property var model
            width: ListView.view.width
            number: index + 1
            trackId: model.trackId
            title: model.title
            version: model.version
            artists: model.artists
            artistRefs: model.artistRefs
            albumId: model.albumId
            album: model.album
            cover: model.cover
            coverLarge: model.coverLarge ?? ""
            uri: model.uri ?? ""
            isEpisode: model.isEpisode ?? false
            showId: model.showId ?? ""
            durationMs: model.durationMs
            explicit: model.explicit
            available: model.available
            addedAt: model.addedAt ?? ""
            addedBy: model.addedBy ?? ""
            addedColumn: page.addedColumn
            playCount: model.playCount ?? 0
            showPlays: page.showPlays
            showAlbum: page.showAlbum
            editable: list.reorderable || (page.editing && !filter.active)
            dragging: list.dragFrom === index
            liked: Library.likesRevision >= 0 && Library.isLiked(model.trackId)
            current: model.trackId === Player.trackId
            highlighted: (page._flashId !== "" && model.trackId === page._flashId) || page._flashRow === index
            // pos — position in the list itself (for "Remove from this playlist"); unknown while searching.
            // TrackRow builds the dict for its menu itself, lazily (trackData()) — get(index) in full
            // (18 roles in a QVariantMap per row) is no longer needed here
            pos: filter.active ? -1 : index
            z: dragging ? 10 : 0
            transform: Translate {
                y: row.dragging ? list.dragOffset : list.shiftFor(row.index)
                Behavior on y {
                    enabled: !row.dragging && list.shiftAnimated && Theme.reducedMotion !== true
                    NumberAnimation { duration: Theme.motion.effectsSlow; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized }
                }
            }
            onActivated: {
                page.trackActivated(index)
                Player.playFrom(page.shownModel, index)
            }
            onLikeClicked: Library.toggleLike(page.shownModel.get(index))
            onRemoveClicked: page.trackRemoved(index)
            onDragStarted: list.beginDrag(index)
            onDragMoved: (dy, sceneY) => list.updateDrag(dy, sceneY)
            onDragFinished: list.endDrag()
        }

        footer: Item {
            id: footerBox
            width: list.width
            height: 120 + (page.model && page.model.loading && page.model.count > 0 ? 64 : 0) + extraLoader.height
            Loader {
                id: extraLoader
                width: parent.width
                active: page.footerExtra !== null
                sourceComponent: page.footerExtra
            }
            LoadingIndicator {
                anchors.horizontalCenter: parent.horizontalCenter
                y: extraLoader.height
                visible: page.model && page.model.loading && page.model.count > 0
                // A long playlist loads in pages over seconds; a spinning indicator repaints the window
                // at monitor rate, so it only spins while the bottom of the list is on screen
                running: footerBox.y < list.contentY + list.height
            }
            Label {
                anchors.horizontalCenter: parent.horizontalCenter
                y: extraLoader.height + 8
                width: Math.min(parent.width - 32, 520)
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.Wrap
                visible: filter.active && filter.count === 0 && !(page.model && page.model.loading)
                // quote filter.query, not searchText: the text must match what the list actually shows
                text: I18n.t("Nothing in this list matches “{0}”", filter.query.trim())
                type: "bodyLarge"
                color: Theme.fgSurfaceVariant
            }
        }
    }

    SmoothScroll { flickable: list; wheelStep: Settings.wheelStep; onUserScrolled: list.userScrolled = true }

    Component {
        id: defaultHero
        RowLayout {
            id: hero
            readonly property alias searchField: searchField
            spacing: page.width < 560 ? 18 : 28
            Behavior on spacing { NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }

            // Cover or a shape with an icon; behind the cover — a slightly bigger shape
            Item {
                Layout.preferredWidth: page.width > 760 ? 184 : page.width < 560 ? 96 : 128
                Layout.preferredHeight: Layout.preferredWidth
                Behavior on Layout.preferredWidth { NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
                Layout.alignment: Qt.AlignTop
                MorphShape {
                    id: coverBackdrop
                    visible: page.heroImage !== ""
                    anchors.centerIn: parent
                    width: parent.width * 1.3
                    height: width
                    shape: "softSquare"
                    color: Theme.surfaceContainerHighest
                }
                TickAnimation {
                    running: coverBackdrop.visible && page.visible && page._settled && !Theme.quiet && Theme.reducedMotion !== true
                             && list.contentY - list.originY < 300
                    onTriggered: coverBackdrop.rotation = (coverBackdrop.rotation + frameTime * 6) % 360
                }
                MorphImage {
                    anchors.fill: parent
                    visible: page.heroImage !== ""
                    source: page.heroImage
                    shape: "softSquare"
                    HoverHandler { id: coverHover }
                }
                MorphShape {
                    anchors.fill: parent
                    visible: page.heroImage === ""
                    shape: page.heroShape
                    color: Theme.primaryContainer
                    Icon {
                        anchors.centerIn: parent
                        name: page.heroIcon
                        size: parent.width * 0.38
                        fill: 1
                        color: Theme.fgPrimaryContainer
                    }
                }
            }

            ColumnLayout {
                Layout.fillWidth: true
                Layout.alignment: Qt.AlignBottom
                spacing: 4
                Label {
                    visible: page.overline !== ""
                    text: page.overline
                    type: "labelLarge"
                    color: Theme.primary
                    font.letterSpacing: 1.2
                }
                Label {
                    Layout.fillWidth: true
                    text: page.title
                    type: page.width > 760 ? "displayMedium" : page.width < 560 ? "headlineMedium" : "displaySmall"
                    weight: 700
                    wrapMode: Text.WordWrap
                    maximumLineCount: 2
                }
                Label {
                    visible: page.description !== ""
                    Layout.fillWidth: true
                    text: page.description
                    type: "bodyLarge"
                    color: Theme.fgSurfaceVariant
                    wrapMode: Text.WordWrap
                    maximumLineCount: 2
                    elide: Text.ElideRight
                }
                Flow {
                    visible: (page.heroArtistRefs ?? []).length > 0
                    Layout.fillWidth: true
                    spacing: 6
                    Repeater {
                        model: page.heroArtistRefs
                        LinkLabel {
                            required property var modelData
                            required property int index
                            text: modelData.name + (index < page.heroArtistRefs.length - 1 ? "," : "")
                            type: "titleLarge"
                            weight: 600
                            onClicked: Catalog.openArtist(modelData.id)
                        }
                    }
                }
                Label {
                    visible: text !== "" && page.heroMeta === null
                    Layout.fillWidth: true
                    text: page.subtitle + (page.model && page.model.totalDurationMs > 0 && page.subtitle !== ""
                                           ? "  ·  " + page._duration(page.model.totalDurationMs) : "")
                    type: "bodyLarge"
                    color: Theme.fgSurfaceVariant
                }
                Loader {
                    Layout.fillWidth: true
                    active: page.heroMeta !== null
                    visible: active
                    sourceComponent: page.heroMeta
                }
                // in a narrow window the buttons wrap instead of stretching the header
                Flow {
                    Layout.fillWidth: true
                    Layout.topMargin: 14
                    spacing: 12
                    visible: page.model !== null && (page.model.count > 0 || page.headerExtra !== null)
                    Button {
                        visible: page.showPlayButtons
                        text: I18n.t("Play")
                        icon: "play_arrow"
                        size: "m"
                        enabled: page.shownModel !== null && page.shownModel.count > 0
                        onClicked: Player.playFrom(page.shownModel, 0)
                    }
                    Button {
                        visible: page.showPlayButtons
                        text: I18n.t("Shuffle play")
                        icon: "shuffle"
                        style: "tonal"
                        size: "m"
                        enabled: page.shownModel !== null && page.shownModel.count > 0
                        onClicked: Player.shuffleFrom(page.shownModel)
                    }
                    Loader {
                        active: page.headerExtra !== null
                        sourceComponent: page.headerExtra
                    }
                    IconButton {
                        visible: page.model !== null && page.model.count > 0
                        icon: "search"
                        size: "m"
                        style: page.searchOpen ? "tonal" : "standard"
                        checkable: true
                        autoToggle: false
                        checked: page.searchOpen
                        onClicked: page.searchOpen ? page.closeSearch() : page.findInPage()
                    }
                }
                Loader {
                    Layout.fillWidth: true
                    Layout.topMargin: 8
                    active: page.heroNote !== null
                    visible: active
                    sourceComponent: page.heroNote
                }
                RowLayout {
                    visible: page.searchOpen
                    Layout.fillWidth: true
                    Layout.topMargin: 10
                    spacing: 16
                    SearchField {
                        id: searchField
                        Layout.fillWidth: true
                        Layout.maximumWidth: 420
                        placeholder: I18n.t("Find in list")
                        text: page.searchText
                        onTextChanged: page.searchText = text
                        Keys.onEscapePressed: page.closeSearch()
                    }
                    Label {
                        visible: filter.active
                        text: I18n.t("{0} of {1}", filter.count, page.model ? page.model.count : 0)
                        type: "bodyMedium"
                        color: Theme.fgSurfaceVariant
                    }
                    Item { Layout.fillWidth: true }
                }
            }
        }
    }

    // First load / empty
    LoadingIndicator {
        anchors.centerIn: parent
        size: 64
        contained: true
        visible: page.model !== null && page.model.loading && page.model.count === 0
    }
    EmptyState {
        anchors.horizontalCenter: parent.horizontalCenter
        anchors.horizontalCenterOffset: -page.contentRightInset / 2
        y: Math.max((list.headerItem ? list.headerItem.height - (list.contentY - list.originY) : 0) + 8,
                    (parent.height - implicitHeight) / 2)
        width: Math.min(parent.width - 64 - page.contentRightInset, 420)
        visible: page.model !== null && !page.model.loading && page.model.count === 0 && page.footerExtra === null
        icon: page.emptyIcon
        title: page.emptyTitle
        text: page.emptyText
        actionText: page.emptyActionText
        actionIcon: page.emptyActionIcon
        onAction: page.emptyAction()
    }
}
