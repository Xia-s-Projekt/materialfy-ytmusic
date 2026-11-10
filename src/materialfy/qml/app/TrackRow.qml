import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Track row: number - cover - title/artists - album - (added by/added at) - (plays) - like -
// duration.
// editable — playlist edit mode: a drag handle on the left, "remove" instead of the heart and ....
Item {
    id: root

    property int number: 0
    property string trackId
    property string title
    property string version
    property string artists
    property var artistRefs: []        // [{ id, name }] — for navigating to the artist
    property string albumId
    property string album
    property string cover
    property string coverLarge
    property string uri
    property bool isEpisode: false
    property string showId
    property int durationMs: 0
    property bool explicit: false
    property bool available: true
    // plays on Spotify (0 — unknown); the column only where showPlays (album, artist)
    // and the row is wide enough
    property real playCount: 0
    property bool showPlays: false
    property bool _playsRoom: width > 600
    property bool liked: false
    property bool current: false       // playing now
    // the selected track is still loading (the engine has not handed it over): the row already shows it, but there is no sound yet
    property bool loading: Player.buffering
    property bool showAlbum: true
    property bool wide: showAlbum && _wideAuto
    // the album column appears with a +/-20 px margin, so it does not flicker right at the boundary when resizing
    property bool _wideAuto: width > 720
    onWidthChanged: {
        _wideAuto = _wideAuto ? width > 700 : width > 740
        _playsRoom = _playsRoom ? width > 580 : width > 620
        _sized = width > 0
    }
    property real _albumT: wide ? 1 : 0
    // The column slides out when the row is stretched; the first width (the row is created zero-width in the layout) — immediately,
    // otherwise every new row would animate the column for half a second and relayout on every frame
    property bool _sized: false
    Component.onCompleted: _sized = width > 0
    Behavior on _albumT { enabled: root._sized; NumberAnimation { duration: 0; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
    property var track: null           // the full track dictionary for the menu; without it — assembled from the fields
    // position in the playlist (for "Remove from this playlist"); -1 — unknown (for example, in search)
    // and then it does not make it into the menu dictionary: Main.qml tells "none" from a number
    property int pos: -1
    property bool highlighted: false   // a "jump to track" flash / after adding

    // Playlist: who added it and when. addedColumn: "" — no column, "by" — "Added by", "at" — "Added at"
    property string addedAt
    property string addedBy
    property string addedColumn
    property bool editable: false
    property bool dragging: false      // the row is lifted and follows the cursor

    signal activated()                 // click — play
    signal likeClicked()               // click on the heart
    signal removeClicked()             // "remove from playlist" (editable)
    signal dragStarted()
    signal dragMoved(real dy, real sceneY)
    signal dragFinished()

    implicitHeight: 64

    function trackData() {
        if (track)
            return track
        const data = {
            trackId: trackId, albumId: albumId, title: title, version: version, artists: artists,
            artistRefs: artistRefs ?? [], album: album, cover: cover, coverLarge: coverLarge, durationMs: durationMs,
            explicit: explicit, available: available, uri: uri, isEpisode: isEpisode, showId: showId,
            addedAt: addedAt, addedBy: addedBy, playCount: playCount
        }
        if (pos >= 0)
            data.pos = pos
        return data
    }

    function openMenu(item, x, y) {
        const p = item.mapToItem(null, x, y)
        Catalog.requestTrackMenu(trackData(), p.x, p.y)
    }
    // the menu at the "..." button: below it, right-aligned to its right edge
    function openMenuAt(button) {
        const p = button.mapToItem(null, 0, 0)
        Catalog.requestTrackMenuAt(trackData(), p.x, p.y, button.width, button.height)
    }

    function _time(ms) {
        const s = Math.round(ms / 1000)
        return Math.floor(s / 60) + ":" + String(s % 60).padStart(2, "0")
    }

    function _date(iso) {
        if (!iso)
            return ""
        const d = new Date(iso)
        if (isNaN(d.getTime()))
            return ""
        const now = new Date()
        const day = 24 * 3600 * 1000
        const start = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime()
        if (d.getTime() >= start)
            return I18n.t("Today")
        if (d.getTime() >= start - day)
            return I18n.t("Yesterday")
        return I18n.shortDate(d)
    }

    readonly property bool _compactAdded: width < 600
    readonly property real _editT: editable ? 1 : 0

    Rectangle {
        id: bg
        anchors.fill: parent
        radius: Theme.shape.large
        color: root.dragging ? Theme.surfaceContainerHighest
             : root.current && !root.editable ? Qt.alpha(Theme.primary, 0.12) : "transparent"
    }
    // the shadow of the lifted row (Elevation is placed next to bg — inside the row, moves with it)
    Elevation { target: bg; level: root.dragging ? 2 : 0 }

    // The flash and everything visible only in edit mode or under the cursor is created on demand: there are dozens of rows in the list,
    // but only one blinks, is edited or hovered
    Loader {
        anchors.fill: parent
        active: root.highlighted
        sourceComponent: Rectangle {
            id: flash
            radius: Theme.shape.large
            color: Theme.primary
            opacity: 0
            SequentialAnimation {
                running: Theme.reducedMotion !== true
                loops: 2
                NumberAnimation { target: flash; property: "opacity"; to: 0.18; duration: Theme.motion.effectsDefault }
                NumberAnimation { target: flash; property: "opacity"; to: 0; duration: Theme.motion.effectsDefault * 2 }
            }
            // reduced motion — no flash: an even highlight while the row is marked
            states: State {
                when: Theme.reducedMotion === true
                PropertyChanges { flash.opacity: 0.12 }
            }
        }
    }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 12 - 8 * root._editT
        anchors.rightMargin: 16
        spacing: 16
        // a region-unavailable track is dimmed, as in the queue (QueueRow)
        opacity: root.available ? 1 : Theme.stateLayer.disabledContent

        // Drag handle: width 0 -> 28 on entering edit mode
        Item {
            id: handle
            Layout.preferredWidth: root.editable ? 28 : 0
            Layout.rightMargin: root.editable ? 0 : -16
            Layout.fillHeight: true
            Behavior on Layout.preferredWidth { enabled: Theme.reducedMotion !== true; NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
            Behavior on Layout.rightMargin { enabled: Theme.reducedMotion !== true; NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
            clip: true
            // from the first entry into edit mode and stays (the icon dims on exit)
            Loader {
                anchors.fill: parent
                active: root.editable || item !== null
                sourceComponent: Item {
                    Icon {
                        anchors.centerIn: parent
                        name: "drag_indicator"
                        size: 22
                        color: handleHover.hovered || root.dragging ? Theme.fgSurface : Theme.fgSurfaceVariant
                        opacity: root.editable ? 1 : 0
                        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }
                    }
                    HoverHandler { id: handleHover; cursorShape: root.dragging ? Qt.ClosedHandCursor : Qt.OpenHandCursor; enabled: root.editable }
                    DragHandler {
                        enabled: root.editable
                        target: null
                        yAxis.enabled: true
                        xAxis.enabled: false
                        grabPermissions: PointerHandler.CanTakeOverFromAnything
                        onActiveChanged: active ? root.dragStarted() : root.dragFinished()
                        onTranslationChanged: if (active) root.dragMoved(translation.y, centroid.scenePosition.y)
                    }
                }
            }
        }

        Label {
            visible: root.number > 0
            Layout.preferredWidth: 28
            text: root.number
            type: "bodyMedium"
            color: Theme.fgSurfaceVariant
            horizontalAlignment: Text.AlignRight
        }

        // the selected track is still loading — an MD3 indicator over the cover; the row has already switched to it
        Item {
            Layout.preferredWidth: 48
            Layout.preferredHeight: 48
            MorphImage {
                anchors.fill: parent
                source: root.cover
                fallbackKind: root.album !== "" ? "album" : "track"
                fallbackArtist: root.artists.split(", ")[0]
                fallbackTitle: root.album !== "" ? root.album : root.title
                shape: "softSquare"
            }
            LoadingIndicator {
                anchors.centerIn: parent
                size: 34
                color: Theme.primary
                visible: root.current && root.loading
                running: visible && root.visible
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            Layout.preferredWidth: 3
            spacing: 2
            RowLayout {
                Layout.fillWidth: true
                spacing: 6
                Label {
                    Layout.fillWidth: true
                    Layout.maximumWidth: Math.ceil(implicitWidth)   // fractional scale: the layout width rounds down
                    text: root.title
                    type: "bodyLarge"
                    weight: 500
                    color: root.current ? Theme.primary : Theme.fgSurface
                }
                Label {
                    visible: root.version !== ""
                    Layout.fillWidth: true
                    Layout.maximumWidth: Math.ceil(implicitWidth)
                    text: root.version
                    type: "bodyLarge"
                    color: Theme.fgSurfaceVariant
                }
                Rectangle {
                    visible: root.explicit
                    implicitWidth: 16
                    implicitHeight: 16
                    radius: 4
                    color: Theme.surfaceContainerHighest
                    Label { anchors.centerIn: parent; text: "E"; type: "labelSmall"; color: Theme.fgSurfaceVariant }
                }
                Item { Layout.fillWidth: true }
            }
            ArtistLinks {
                Layout.fillWidth: true
                refs: root.artistRefs ?? []
                text: root.artists
                type: "bodyMedium"
                color: Theme.fgSurfaceVariant
            }
        }

        LinkLabel {
            visible: root._albumT > 0.01
            opacity: root._albumT
            Layout.fillWidth: true
            Layout.preferredWidth: 2 * root._albumT
            linkEnabled: root.albumId !== ""
            onClicked: Catalog.openAlbum(root.albumId)
            text: root.album
            type: "bodyMedium"
            color: Theme.fgSurfaceVariant
        }

        // "Added by" (avatar + name) / "Added at" (date); in a narrow row — avatar only, and the date is hidden:
        // the space is needed for the title
        RowLayout {
            visible: root.addedColumn !== "" && !(root.addedColumn === "at" && root._compactAdded)
            Layout.preferredWidth: root.addedColumn === "by" && root._compactAdded ? 28 : 132
            Layout.maximumWidth: Layout.preferredWidth
            spacing: 8
            Avatar {
                visible: root.addedColumn === "by" && root.addedBy !== ""
                size: 24
                name: root.addedBy
            }
            Label {
                visible: !(root.addedColumn === "by" && root._compactAdded)
                Layout.fillWidth: true
                text: root.addedColumn === "by" ? root.addedBy : root._date(root.addedAt)
                type: "bodyMedium"
                color: Theme.fgSurfaceVariant
            }
        }

        // Plays: the full number with digit grouping
        Label {
            visible: root.showPlays && root._playsRoom
            Layout.preferredWidth: 112
            text: root.playCount > 0 ? I18n.grouped(root.playCount) : ""
            type: "bodyMedium"
            color: Theme.fgSurfaceVariant
            horizontalAlignment: Text.AlignRight
            font.features: ({ "tnum": 1 })
        }

        // Heart and ... — in view mode; "remove" — in edit mode (cross-fade)
        Item {
            implicitWidth: root.editable ? 32 : 32 + 16 + 32
            implicitHeight: 32

            Row {
                visible: opacity > 0
                opacity: 1 - root._editT
                Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }
                spacing: 16

                // Heart: filled when liked, outline on row hover
                Item {
                    width: 32
                    height: 32
                    Icon {
                        anchors.centerIn: parent
                        visible: root.available
                        name: "favorite"
                        size: 20
                        fill: root.liked ? 1 : 0
                        color: root.liked ? Theme.primary : heartArea.containsMouse ? Theme.fgSurface : Theme.fgSurfaceVariant
                        opacity: root.liked || rowHover.hovered ? 1 : 0
                        scale: heartArea.pressed ? 0.85 : 1
                        Behavior on scale { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutBack } }
                    }
                    StateLayer {
                        id: heartArea
                        anchors.fill: parent
                        radius: width / 2
                        visible: root.available && !root.editable && (root.liked || rowHover.hovered)
                        onClicked: root.likeClicked()
                    }
                }
                // "..." — from the first hover over the row
                Loader {
                    width: 32
                    height: 32
                    active: rowHover.hovered || item !== null
                    sourceComponent: IconButton {
                        id: moreButton
                        icon: "more_horiz"
                        size: "xs"
                        iconSize: 26
                        iconWeight: 700
                        enabled: !root.editable
                        opacity: rowHover.hovered || pressed ? 1 : 0
                        onClicked: root.openMenuAt(moreButton)
                    }
                }
            }
            // "remove" — from the first entry into edit mode
            Loader {
                anchors.right: parent.right
                width: 32
                height: 32
                active: root.editable || item !== null
                visible: opacity > 0
                opacity: root._editT
                Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }
                sourceComponent: IconButton {
                    icon: "remove_circle"
                    size: "xs"
                    iconColor: Theme.fgSurfaceVariant
                    onClicked: root.removeClicked()
                }
            }
        }

        Label {
            visible: root.durationMs > 0
            Layout.preferredWidth: root.durationMs > 0 ? 44 : 0
            text: root.durationMs > 0 ? root._time(root.durationMs) : ""
            type: "bodyMedium"
            color: Theme.fgSurfaceVariant
            horizontalAlignment: Text.AlignRight
        }
    }

    HoverHandler { id: rowHover }

    // Dragging into the queue (outside edit mode)
    QueueDragSource {
        anchors.fill: parent
        z: -2
        source: root
        enabled: !root.editable && root.available && root.uri !== ""
        title: root.title
        cover: root.cover
    }

    TapHandler {
        acceptedButtons: Qt.RightButton
        onTapped: (point) => root.openMenu(root, point.position.x, point.position.y)
    }

    // Under the content: clicks on the heart go to it, clicks on the rest of the row — here
    StateLayer {
        z: -1
        anchors.fill: parent
        radius: Theme.shape.large
        dragged: root.dragging
        onClicked: if (!root.editable) root.activated()
    }
}
