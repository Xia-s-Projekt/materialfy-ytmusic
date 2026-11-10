import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core
import ".."

// Podcast: header (cover, publisher, "Play latest" / "Continue" / "Pause", subscribe),
// description, episodes with filters and sorting (local), loading more in pages of 50.
Page {
    id: page

    property string showId
    readonly property QtObject show: Catalog.show(showId)
    readonly property var info: show ? show.info : ({})
    readonly property var episodes: show ? show.episodes : null
    readonly property bool compact: width < 840     // 900 px window (rail) or 1280 with the right panel
    readonly property bool saved: Library.savedRevision >= 0 && (info.uri ?? "") !== "" ? Library.isSaved(info.uri) : info.saved === true

    property string filter: "all"          // all | unfinished | new | saved
    property bool newestFirst: true
    property bool descriptionOpen: false

    title: info.name ?? ""
    showHeader: false
    readonly property bool heroTitle: true     // the title is in the header; in the top bar — after scrolling

    readonly property var filters: [
        { key: "all", text: I18n.t("All") }, { key: "unfinished", text: I18n.t("In progress") },
        { key: "new", text: I18n.t("New") }, { key: "saved", text: I18n.t("Saved") }
    ]

    // ---- Episodes: filter and order on the QML side ----
    property int _revision: 0
    Connections {
        target: page.episodes
        function onCountChanged() { page._revision++ }
        function onModelReset() { page._revision++ }
        function onDataChanged() { page._revision++ }
    }
    readonly property var allEpisodes: {
        const _ = _revision
        const out = []
        if (!episodes)
            return out
        for (let i = 0; i < episodes.count; i++)
            out.push(episodes.get(i))
        return out
    }
    readonly property var shownEpisodes: {
        const _ = Library.savedRevision
        let list = allEpisodes.filter(e => {
            switch (page.filter) {
            case "unfinished": return (e.resumeMs ?? 0) > 0 && e.fullyPlayed !== true
            case "new": return (e.resumeMs ?? 0) === 0 && e.fullyPlayed !== true
            case "saved": return Library.isSaved(e.uri ?? "")
            }
            return true
        })
        // from the server episodes arrive newest first
        if (!newestFirst)
            list = list.slice().reverse()
        return list
    }
    readonly property var playingEpisode: Player.isEpisode ? allEpisodes.find(e => e.uri === Player.uri) ?? null : null
    readonly property var resumeEpisode: allEpisodes.find(e => (e.resumeMs ?? 0) > 0 && e.fullyPlayed !== true) ?? null
    readonly property bool hasMore: episodes !== null && episodes.count < (show ? show.total : 0)

    function mainAction() {
        if (playingEpisode)
            Player.togglePlay()
        else if (resumeEpisode)
            Player.playEpisode(resumeEpisode.uri, resumeEpisode.resumeMs)
        else if (allEpisodes.length > 0)
            Player.playEpisode(allEpisodes[0].uri, allEpisodes[0].fullyPlayed ? 0 : (allEpisodes[0].resumeMs ?? 0))
    }
    // the next page is loading (ShowData.loadingMore; calling loadMore() again is safe)
    readonly property bool loadingMore: show !== null && show.loadingMore === true
    function loadMore() {
        if (hasMore && show && !show.loading && !loadingMore)
            show.loadMore()
    }
    // load more when less than a screen is left to the end of the list
    Connections {
        target: page.flickable
        function onContentYChanged() {
            if (page.hasMore && page.flickable.contentY > page.flickable.maxY - page.height)
                page.loadMore()
        }
    }

    underlay: HeroShapes {
        width: page.width
        active: page.visible
        headerHeight: hero.height + 24
        scroll: page.flickable.contentY
        contentX: page.sideMargin - 16
        contentWidth: page.width - 2 * contentX
        seed: page.info.name ?? ""
        shapesOpacity: 0.6
    }

    // ---- Header ----
    Item {
        id: hero
        visible: (page.info.name ?? "") !== "" || (page.show !== null && page.show.loading)
        Layout.fillWidth: true
        Layout.preferredHeight: heroRow.implicitHeight + 32

        RowLayout {
            id: heroRow
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            spacing: page.compact ? 20 : 32

            MorphImage {
                Layout.preferredWidth: page.compact ? 150 : 200
                Layout.preferredHeight: Layout.preferredWidth
                Layout.alignment: Qt.AlignTop
                Behavior on Layout.preferredWidth { NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
                source: page.info.coverLarge || page.info.cover || ""
                shape: "softSquare"
                duration: Theme.motion.spatialSlow
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 4
                Label { text: I18n.t("Podcast").toUpperCase(); type: "labelMedium"; color: Theme.primary; font.letterSpacing: 1.2 }
                Label {
                    Layout.fillWidth: true
                    text: page.info.name ?? ""
                    type: page.compact ? "headlineMedium" : "displaySmall"
                    weight: 600
                    wrapMode: Text.WordWrap
                    maximumLineCount: 2
                }
                Label {
                    Layout.fillWidth: true
                    visible: text !== ""
                    text: page.info.publisher ?? ""
                    type: "titleMedium"
                    font.pixelSize: 18
                }
                Label {
                    Layout.fillWidth: true
                    visible: text !== ""
                    text: {
                        const n = (page.show && page.show.total) || (page.info.totalEpisodes ?? 0)
                        return [n > 0 ? I18n.n(n, "{0} episode", "{0} episodes") : "", page.info.explicit ? "18+" : ""].filter(x => x).join(" · ")
                    }
                    type: "bodyMedium"
                    color: Theme.fgSurfaceVariant
                }
                Flow {
                    Layout.fillWidth: true
                    Layout.topMargin: 14
                    spacing: 12
                    visible: (page.info.name ?? "") !== ""
                    Button {
                        size: "m"
                        enabled: page.allEpisodes.length > 0
                        icon: page.playingEpisode && Player.playing ? "pause" : "play_arrow"
                        text: page.playingEpisode ? (Player.playing ? I18n.t("Pause") : I18n.t("Continue"))
                            : page.resumeEpisode ? (page.resumeEpisode.number ? I18n.t("Continue · ep. {0}", page.resumeEpisode.number) : I18n.t("Continue"))
                            : I18n.t("Play latest")
                        onClicked: page.mainAction()
                    }
                    IconButton {
                        size: "m"
                        style: page.saved ? "tonal" : "standard"
                        checkable: true
                        checked: page.saved
                        autoToggle: false
                        icon: page.saved ? "library_add_check" : "library_add"
                        Accessible.name: page.saved ? I18n.t("In library") : I18n.t("Follow")
                        onClicked: Library.toggleSaved(page.info.uri ?? "")
                    }
                    IconButton {
                        size: "m"
                        icon: "share"
                        onClicked: {
                            Catalog.copyLink(Catalog.uriUrl(page.info.uri ?? ""))
                            snackbar.show(I18n.t("Link copied"))
                        }
                    }
                }
            }
        }
    }

    // ---- Description ----
    ColumnLayout {
        Layout.fillWidth: true
        Layout.maximumWidth: 880
        visible: (page.info.description ?? "") !== ""
        spacing: 4
        Item {
            Layout.fillWidth: true
            implicitHeight: page.descriptionOpen ? fullText.implicitHeight : Math.min(fullText.implicitHeight, 2 * 24)
            Behavior on implicitHeight { NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
            clip: true
            Label {
                id: fullText
                width: parent.width
                text: page.info.description ?? ""
                textFormat: Text.PlainText
                type: "bodyLarge"
                color: Theme.fgSurfaceVariant
                wrapMode: Text.WordWrap
                elide: Text.ElideNone
            }
        }
        Button {
            visible: fullText.implicitHeight > 2 * 24 + 2
            text: page.descriptionOpen ? I18n.t("Show less") : I18n.t("More")
            style: "text"
            size: "xs"
            onClicked: page.descriptionOpen = !page.descriptionOpen
        }
    }

    // ---- Episodes ----
    SectionHeader {
        visible: (page.info.name ?? "") !== ""
        text: I18n.t("Episodes")
    }
    RowLayout {
        Layout.fillWidth: true
        Layout.topMargin: -8
        visible: (page.info.name ?? "") !== ""
        spacing: 8
        Flickable {
            Layout.fillWidth: true
            implicitHeight: 32
            contentWidth: chips.implicitWidth
            flickableDirection: Flickable.HorizontalFlick
            boundsBehavior: Flickable.StopAtBounds
            interactive: contentWidth > width
            clip: true
            Row {
                id: chips
                spacing: 8
                Repeater {
                    model: page.filters
                    Chip {
                        required property var modelData
                        text: modelData.text
                        selected: page.filter === modelData.key
                        onClicked: page.filter = modelData.key
                    }
                }
            }
        }
        Chip {
            icon: "swap_vert"
            // on a narrow page — icon only: the filters need the space more
            text: page.width < 640 ? "" : page.newestFirst ? I18n.t("Newest first") : I18n.t("Oldest first")
            onClicked: page.newestFirst = !page.newestFirst
        }
    }

    ColumnLayout {
        Layout.fillWidth: true
        spacing: 0
        Repeater {
            model: page.shownEpisodes
            ColumnLayout {
                id: epItem
                required property var modelData
                required property int index
                Layout.fillWidth: true
                spacing: 0
                WavyDivider {
                    visible: epItem.index > 0
                    Layout.fillWidth: true
                    Layout.leftMargin: (page.compact ? 64 : 88) + 28
                    Layout.rightMargin: 12
                    Layout.preferredHeight: 8
                }
                EpisodeRow {
                    Layout.fillWidth: true
                    episode: epItem.modelData
                    compact: page.compact
                }
            }
        }
    }

    // loading: three placeholder rows
    ColumnLayout {
        Layout.fillWidth: true
        visible: page.show !== null && page.show.loading
        spacing: 16
        Repeater {
            model: 3
            RowLayout {
                Layout.fillWidth: true
                spacing: 16
                Rectangle {
                    Layout.preferredWidth: page.compact ? 64 : 88
                    Layout.preferredHeight: Layout.preferredWidth
                    radius: Theme.shape.large
                    color: Theme.surfaceContainerHigh
                }
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 10
                    Rectangle { Layout.preferredWidth: 120; Layout.preferredHeight: 12; radius: 6; color: Theme.surfaceContainerHigh }
                    Rectangle { Layout.fillWidth: true; Layout.maximumWidth: 420; Layout.preferredHeight: 18; radius: 9; color: Theme.surfaceContainerHigh }
                    Rectangle { Layout.fillWidth: true; Layout.maximumWidth: 560; Layout.preferredHeight: 12; radius: 6; color: Theme.surfaceContainerHigh }
                }
            }
        }
    }

    EmptyState {
        Layout.alignment: Qt.AlignHCenter
        Layout.topMargin: 24
        visible: page.show !== null && !page.show.loading && page.show.errorText === ""
                 && page.allEpisodes.length > 0 && page.shownEpisodes.length === 0
        icon: page.filter === "saved" ? "bookmark" : "podcasts"
        title: page.filter === "unfinished" ? I18n.t("No episodes in progress")
             : page.filter === "new" ? I18n.t("No new episodes")
             : I18n.t("No saved episodes")
        text: page.hasMore ? I18n.t("Not all episodes are loaded — scroll down or load more") : ""
        actionText: page.hasMore ? I18n.t("Load more") : ""
        actionIcon: "expand_more"
        onAction: page.loadMore()
    }
    EmptyState {
        Layout.alignment: Qt.AlignHCenter
        Layout.topMargin: 48
        visible: page.show !== null && !page.show.loading && page.show.errorText !== ""
        icon: "cloud_off"
        title: I18n.t("Podcast didn't load")
        text: page.show ? page.show.errorText : ""
    }

    LoadingIndicator {
        Layout.alignment: Qt.AlignHCenter
        Layout.topMargin: 8
        visible: page.loadingMore
        size: 40
    }
    Button {
        Layout.alignment: Qt.AlignHCenter
        visible: page.hasMore && page.show !== null && !page.show.loading && !page.loadingMore && page.shownEpisodes.length > 0
        text: I18n.t("More episodes")
        icon: "expand_more"
        style: "text"
        onClicked: page.loadMore()
    }

    Snackbar {
        id: snackbar
        parent: page
        anchors.fill: parent
        z: 50
    }
}
