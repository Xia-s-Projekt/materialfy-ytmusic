import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core
import ".."

// Artist: header (a wide photo as the background, if there is one; the photo in a shape, monthly listeners, followers, Listen / Radio / Follow), popular tracks,
// albums and singles, releases with appearances, "About the artist", similar artists.
Page {
    id: page

    property string artistId
    readonly property QtObject artist: Catalog.artist(artistId)
    readonly property var info: artist ? artist.info : ({})
    property bool allPopular: false
    property bool allAlbums: false

    title: info.name ?? ""
    showHeader: false
    readonly property bool heroTitle: true     // the name is in the header; in the top bar — after scrolling

    readonly property bool followed: Library.savedRevision >= 0 && (info.uri ?? "") !== "" ? Library.isSaved(info.uri) : info.followed === true

    // 12.3M / 450K / 8,765 — I18n.nCompact (after the million/thousand abbreviation — the genitive plural)
    function followersText(n) {
        return n > 0 ? I18n.nCompact(n, "{0} follower", "{0} followers") : ""
    }
    function listenersText(n) {
        return n > 0 ? I18n.nCompact(n, "{0} monthly listener", "{0} monthly listeners") : ""
    }

    // The biography arrives without HTML (sf-services), but just in case the tags are stripped here too
    readonly property string biography: (info.biography ?? "").replace(/<[^>]*>/g, "").trim()
    property bool bioOpen: false
    readonly property int bioLines: 4

    // The artist wide header — the page header background; until it loads or if it is absent — shapes, as before
    readonly property string headerImage: info.headerImage ?? ""
    readonly property bool hasBanner: headerImage !== "" && bannerImage.status !== Image.Error
    readonly property bool bannerShown: bannerImage.status === Image.Ready

    underlay: [
        HeroShapes {
            width: page.width
            active: page.visible && !page.bannerShown
            headerHeight: hero.height + 24
            scroll: page.flickable.contentY
            contentX: page.sideMargin - 16
            contentWidth: page.width - 2 * contentX
            seed: page.info.name ?? ""
            shapesOpacity: 0.6
        },
        // The full-width photo: goes under the transparent top bar, at the bottom it dissolves into the surface.
        // On top — a scrim of the surface color: the header text is readable in both light and dark themes.
        Item {
            id: banner
            readonly property real above: 64      // the top bar over the page
            readonly property real below: 120     // reserve under the header for the dissolve
            readonly property color tone: Theme.surfaceContainer
            // The layer height — in 128 px steps, below the photo — an empty strip (cut): while the header grows,
            // the layer texture is not recreated every frame (the D3D11 driver accumulates freed textures)
            readonly property real contentHeight: above + 8 + hero.height + below
            width: page.width
            height: Math.ceil(contentHeight / 128) * 128
            y: -above - page.flickable.contentY
            visible: page.hasBanner && page.visible && page.flickable.contentY < contentHeight
            opacity: page.bannerShown ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsSlow } }
            clip: true

            layer.enabled: visible
            layer.effect: ShaderEffect {
                property real fadeTop: 24 / Math.max(1, banner.height)
                property real fadeBottom: banner.below * 1.3 / Math.max(1, banner.height)
                property real cut: 1 - banner.contentHeight / Math.max(1, banner.height)
                fragmentShader: Qt.resolvedUrl("../shaders/edgefade.frag.qsb")
            }

            Image {
                id: bannerImage
                width: parent.width
                height: banner.contentHeight
                // no parallax: an offset from contentY redrew the banner layer on every scroll frame
                source: page.headerImage
                // decoded to the on-screen size (PreserveAspectCrop — with margin up to full cover), in 256 px steps
                sourceSize: Qt.size(Math.ceil(banner.width * Screen.devicePixelRatio / 256) * 256,
                                    Math.ceil(banner.height * Screen.devicePixelRatio / 256) * 256)
                fillMode: Image.PreserveAspectCrop
                asynchronous: true
            }
            // at the top — dim slightly under the bar buttons, at the bottom of the header — almost the surface
            Rectangle {
                width: parent.width
                height: banner.contentHeight
                gradient: Gradient {
                    GradientStop { position: 0.0; color: Qt.alpha(banner.tone, 0.7) }
                    GradientStop { position: 0.12; color: Qt.alpha(banner.tone, 0.5) }
                    GradientStop { position: 0.28; color: Qt.alpha(banner.tone, 0.3) }
                    GradientStop { position: 0.5; color: Qt.alpha(banner.tone, 0.7) }
                    GradientStop { position: 0.8; color: Qt.alpha(banner.tone, 0.9) }
                    GradientStop { position: 1.0; color: Qt.alpha(banner.tone, 0.95) }
                }
            }
            // on the left, under the header text, denser; on the right the photo stays bright
            Rectangle {
                width: parent.width
                height: banner.contentHeight
                gradient: Gradient {
                    orientation: Gradient.Horizontal
                    GradientStop { position: 0.0; color: Qt.alpha(banner.tone, 0.65) }
                    GradientStop { position: 0.6; color: Qt.alpha(banner.tone, 0.3) }
                    GradientStop { position: 1.0; color: Qt.alpha(banner.tone, 0.0) }
                }
            }
        }
    ]

    // ---- Header ----
    Item {
        id: hero
        Layout.fillWidth: true
        // with a photo the header is taller: the name and buttons drop to the bottom of the image
        Layout.preferredHeight: page.hasBanner ? Math.max(heroRow.implicitHeight + 32, Math.round(Math.min(440, Math.max(260, page.width * 0.3))))
                                               : heroRow.implicitHeight + 32

        RowLayout {
            id: heroRow
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            anchors.bottomMargin: 16
            spacing: page.width < 560 ? 20 : 32
            Behavior on spacing { NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }

            MorphImage {
                Layout.preferredWidth: page.width > 760 ? 208 : page.width < 560 ? 104 : 144
                Layout.preferredHeight: Layout.preferredWidth
                Behavior on Layout.preferredWidth { NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
                source: page.info.coverLarge || page.info.cover || ""
                fallbackKind: "artist"
                fallbackArtist: page.info.name ?? ""
                shape: page.playingHere && Player.playing ? "softSquare" : "circle"
                duration: Theme.motion.spatialSlow
            }

            ColumnLayout {
                Layout.fillWidth: true
                spacing: 4
                Label { text: I18n.t("ARTIST"); type: "labelLarge"; color: Theme.primary; font.letterSpacing: 1.2 }
                Label {
                    Layout.fillWidth: true
                    text: page.info.name ?? ""
                    type: page.width > 760 ? "displayMedium" : page.width < 560 ? "headlineMedium" : "displaySmall"
                    weight: 700
                    wrapMode: Text.WordWrap
                    maximumLineCount: 2
                }
                Label {
                    visible: text !== ""
                    Layout.fillWidth: true
                    text: page.listenersText(page.info.listeners ?? 0)
                    type: "titleMedium"
                }
                Label {
                    visible: text !== ""
                    Layout.fillWidth: true
                    text: [page.followersText(page.info.followers ?? 0), page.info.genres ?? ""].filter(x => x).join("  ·  ")
                    type: "bodyLarge"
                    color: Theme.fgSurfaceVariant
                }
                // in a narrow window the buttons wrap, rather than stretching the header
                Flow {
                    Layout.fillWidth: true
                    Layout.topMargin: 14
                    spacing: 12
                    visible: (page.info.name ?? "") !== ""
                    Button {
                        visible: page.artist && page.artist.popular.count > 0
                        text: I18n.t("Play")
                        icon: "play_arrow"
                        size: "m"
                        onClicked: Player.playFrom(page.artist.popular, 0)
                    }
                    Button {
                        text: I18n.t("Artist’s radio")
                        icon: "radio"
                        style: "tonal"
                        size: "m"
                        enabled: !Radio.locked
                        onClicked: Radio.playStation("artist:" + page.artistId, page.info.name ?? "")
                    }
                    IconButton {
                        Accessible.name: page.followed ? I18n.t("Following") : I18n.t("Follow")
                        icon: page.followed ? "how_to_reg" : "person_add"
                        style: page.followed ? "tonal" : "standard"
                        size: "m"
                        checkable: true
                        checked: page.followed
                        autoToggle: false
                        onClicked: Library.toggleSaved(page.info.uri ?? "")
                    }
                }
            }
        }
    }

    readonly property bool playingHere: Radio.active && Radio.stationUri !== "" && Radio.stationUri.indexOf(artistId) >= 0
                                        || artist && Player.trackId !== "" && artist.popular.find("trackId", Player.trackId) >= 0

    LoadingIndicator {
        Layout.alignment: Qt.AlignHCenter
        Layout.topMargin: 48
        size: 64
        contained: true
        visible: page.artist && page.artist.loading
    }
    EmptyState {
        Layout.alignment: Qt.AlignHCenter
        Layout.topMargin: 48
        visible: page.artist && page.artist.errorText !== ""
        icon: "person_off"
        title: I18n.t("Couldn't load artist")
        text: page.artist ? page.artist.errorText : ""
    }

    // ---- Popular tracks ----
    SectionHeader {
        visible: page.artist && page.artist.popular.count > 0
        text: I18n.t("Popular tracks")
        actionText: page.artist && page.artist.popular.count > 5 ? (page.allPopular ? I18n.t("Show less") : I18n.t("All")) : ""
        onAction: page.allPopular = !page.allPopular
    }
    ColumnLayout {
        Layout.fillWidth: true
        Layout.topMargin: -12
        spacing: 0
        Repeater {
            model: page.artist ? page.artist.popular : null
            TrackRow {
                required property int index
                required property var model
                visible: page.allPopular || index < 5
                Layout.fillWidth: true
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
                durationMs: model.durationMs
                explicit: model.explicit
                available: model.available
                playCount: model.playCount ?? 0
                showPlays: true
                liked: Library.likesRevision >= 0 && Library.isLiked(model.trackId)
                current: model.trackId === Player.trackId
                onActivated: Player.playFrom(page.artist.popular, index)
                onLikeClicked: Library.toggleLike(page.artist.popular.get(index))
                track: page.artist.popular.get(index)
            }
        }
    }

    // ---- Albums ----
    SectionHeader {
        visible: page.artist && page.artist.albums.count > 0
        text: I18n.t("Albums and singles")
        actionText: page.artist && page.artist.albums.count > tilesPerRow.value * 2 ? (page.allAlbums ? I18n.t("Show less") : I18n.t("All")) : ""
        onAction: page.allAlbums = !page.allAlbums
    }
    QtObject {
        id: tilesPerRow
        readonly property int value: Math.max(1, Math.floor((page.width - 2 * page.sideMargin + 20) / (168 + 20)))
    }
    // Tiles — via a Loader: hidden ones (beyond "All") are not created; for a big artist that is hundreds of covers
    Flow {
        Layout.fillWidth: true
        spacing: 20
        Repeater {
            model: page.artist ? page.artist.albums : null
            Loader {
                id: slot
                required property int index
                required property var model
                active: page.allAlbums || index < tilesPerRow.value * 2
                visible: active
                sourceComponent: MediaTile {
                    cover: slot.model.cover
                    coverKind: "album"
                    coverArtist: page.info.name ?? ""
                    coverTitle: slot.model.title
                    title: slot.model.title
                    // the first tile — the newest release (the service puts it first): the label explains
                    // why it is first. Without a year there is no label — meaning the artist has no fresh release
                    subtitle: [slot.index === 0 && slot.model.year > 0 ? I18n.t("Latest release") : "",
                               slot.model.year || "",
                               slot.model.kind ? I18n.any(slot.model.kind) : ""].filter(x => x).join(" · ")
                    explicit: slot.model.explicit
                    // "Listen" on hover, as on the home page: the playing album is paused
                    readonly property string albumUri: slot.model.uri || ("ytmusic:album:" + slot.model.albumId)
                    showPlay: true
                    playing: Player.playing && Player.contextUri === albumUri
                    onPlayClicked: Player.contextUri === albumUri ? Player.togglePlay() : Player.playContext(albumUri, "")
                    onClicked: Catalog.openAlbum(slot.model.albumId)
                }
            }
        }
    }

    SectionHeader {
        visible: page.artist && page.artist.alsoAlbums.count > 0
        text: I18n.t("Appears on")
    }
    Flow {
        Layout.fillWidth: true
        spacing: 20
        Repeater {
            model: page.artist ? page.artist.alsoAlbums : null
            Loader {
                id: slot
                required property int index
                required property var model
                active: index < tilesPerRow.value
                visible: active
                sourceComponent: MediaTile {
                    cover: slot.model.cover
                    coverKind: "album"
                    coverArtist: (slot.model.artists ?? "").split(", ")[0]
                    coverTitle: slot.model.title
                    title: slot.model.title
                    subtitle: slot.model.artists
                    explicit: slot.model.explicit
                    // "Listen" on hover, as on the home page: the playing album is paused
                    readonly property string albumUri: slot.model.uri || ("ytmusic:album:" + slot.model.albumId)
                    showPlay: true
                    playing: Player.playing && Player.contextUri === albumUri
                    onPlayClicked: Player.contextUri === albumUri ? Player.togglePlay() : Player.playContext(albumUri, "")
                    onClicked: Catalog.openAlbum(slot.model.albumId)
                }
            }
        }
    }

    // ---- About the artist ----
    SectionHeader {
        visible: page.biography !== ""
        text: I18n.t("About")
    }
    ColumnLayout {
        Layout.fillWidth: true
        Layout.maximumWidth: 880
        Layout.topMargin: -12
        visible: page.biography !== ""
        spacing: 4
        Item {
            readonly property real clamped: page.bioLines * Theme.type.bodyLarge.line
            Layout.fillWidth: true
            implicitHeight: page.bioOpen ? bioText.implicitHeight : Math.min(bioText.implicitHeight, clamped)
            Behavior on implicitHeight { NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
            clip: true
            Label {
                id: bioText
                width: parent.width
                text: page.biography
                textFormat: Text.PlainText
                type: "bodyLarge"
                color: Theme.fgSurfaceVariant
                wrapMode: Text.WordWrap
                elide: Text.ElideNone
                verticalAlignment: Text.AlignTop
            }
        }
        Button {
            visible: bioText.implicitHeight > page.bioLines * Theme.type.bodyLarge.line + 2
            text: page.bioOpen ? I18n.t("Show less") : I18n.t("More")
            style: "text"
            size: "xs"
            onClicked: page.bioOpen = !page.bioOpen
        }
    }

    // ---- Similar ----
    SectionHeader {
        visible: page.artist && page.artist.similar.count > 0
        text: I18n.t("Fans also like")
    }
    Flow {
        Layout.fillWidth: true
        spacing: 20
        Repeater {
            model: page.artist ? page.artist.similar : null
            Loader {
                id: slot
                required property int index
                required property var model
                active: index < tilesPerRow.value
                visible: active
                sourceComponent: MediaTile {
                    size: 148
                    round: true
                    placeholderIcon: "person"
                    cover: slot.model.cover
                    coverKind: "artist"
                    coverArtist: slot.model.name
                    title: slot.model.name
                    onClicked: Catalog.openArtist(slot.model.artistId)
                }
            }
        }
    }
}
