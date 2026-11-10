import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// The floating mini-player pill (bottom of the content panel).
Item {
    id: root

    readonly property Item coverItem: coverBox
    // narrow window: one line — cover, track, "play" + "next", like and "⋯";
    // modes, volume and panels — in the fullscreen player
    property bool compact: false
    signal openNowPlaying(point coverCenter)   // cover center in window coordinates — the blob grows from here

    implicitHeight: 72

    // In the narrow pill the track needs the space more: as it narrows, "⋯" goes away (the menu stays on right click),
    // volume (it exists in the fullscreen player) and shuffle with repeat
    // with a ±16 px margin: right at the boundary the buttons do not flicker back and forth
    property bool roomForMenu: width >= 880
    property bool roomForVolume: width >= 760
    property bool roomForModes: width >= 640
    // the name of another device in the Connect button; already — icon only (the name is under the track title)
    property bool roomForDeviceName: width >= 900
    onWidthChanged: {
        const room = (now, edge) => now ? width >= edge - 16 : width >= edge + 16
        roomForDeviceName = room(roomForDeviceName, 900)
        roomForMenu = room(roomForMenu, 880)
        roomForVolume = room(roomForVolume, 760)
        roomForModes = room(roomForModes, 640)
    }

    // A button that leaves as the pill narrows: its space collapses with an animation, and the button itself fades and shrinks
    component Fold: Item {
        id: fold
        property bool shown: true
        default property alias content: slot.data
        readonly property real full: slot.childrenRect.width
        implicitHeight: slot.childrenRect.height
        Layout.preferredWidth: shown ? full : 0
        visible: Layout.preferredWidth > 0.5
        clip: true
        Behavior on Layout.preferredWidth { NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
        Item {
            id: slot
            anchors.centerIn: parent
            width: childrenRect.width
            height: childrenRect.height
            opacity: fold.shown ? 1 : 0
            scale: fold.shown ? 1 : 0.6
            Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsFast } }
            Behavior on scale { NumberAnimation { duration: Theme.motion.spatialDefault; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasized } }
        }
    }

    // The position arrives ~5 times a second (from another device — by polling); between updates it moves smoothly.
    readonly property alias progress: playerProgress.value
    PlayerProgress { id: playerProgress; active: seekBar.visible && !seekBar.dragging }

    function _time(ms) {
        const s = Math.max(0, Math.round(ms / 1000))
        return Math.floor(s / 60) + ":" + String(s % 60).padStart(2, "0")
    }

    Rectangle {
        id: bg
        anchors.fill: parent
        radius: height / 2
        color: Theme.surfaceContainerHighest
    }
    Elevation { target: bg; level: 2 }

    RowLayout {
        anchors.fill: parent
        anchors.leftMargin: 12
        anchors.rightMargin: 16
        spacing: 4

        // Controls
        // For a podcast episode, instead of shuffle and repeat — "−10 s" and "+30 s"
        Fold { shown: root.roomForModes && !root.compact; IconButton {
            icon: Player.isEpisode ? "replay_10" : "shuffle"
            checkable: !Player.isEpisode
            autoToggle: false
            checked: !Player.isEpisode && Player.shuffle
            enabled: Player.hasTrack && (Player.isEpisode || Player.source !== "radio")
            onClicked: {
                if (Player.isEpisode)
                    Player.seekRelative(-10000)
                else
                    Player.shuffle = !Player.shuffle
            }
        } }
        IconButton { icon: "skip_previous"; iconColor: Theme.fgSurface; enabled: Player.hasTrack; onClicked: Player.previous() }
        Item {
            implicitWidth: 52
            implicitHeight: 52
            PlayButton {
                anchors.fill: parent
                size: 52
                playing: Player.playing
                loading: Player.buffering
                spin: Player.isLocal === true      // the remote does not redraw the window every frame
                enabled: Player.hasTrack
                onClicked: Player.togglePlay()
            }
        }
        IconButton { icon: "skip_next"; iconColor: Theme.fgSurface; enabled: Player.hasTrack; onClicked: Player.next() }
        Fold { shown: root.roomForModes && !root.compact; IconButton {
            icon: Player.isEpisode ? "forward_30" : Player.repeat === "one" ? "repeat_one" : "repeat"
            checkable: !Player.isEpisode
            autoToggle: false
            checked: !Player.isEpisode && Player.repeat !== "off"
            enabled: Player.hasTrack
            onClicked: Player.isEpisode ? Player.seekRelative(30000) : Player.cycleRepeat()
        } }

        // Track
        Item {
            Layout.fillWidth: true
            Layout.fillHeight: true
            Layout.leftMargin: 12
            Layout.rightMargin: 12
            // in the narrowest layout the text does not run under the like button
            clip: true

            // Wheel over the cover and title — volume, as in mini mode: a notch is ±5 %, trackpad smoothly
            WheelHandler {
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad
                onWheel: event => {
                    if (Player.remote === true && Player.canSetVolume === false)
                        return
                    const touchpad = event.pixelDelta.y !== 0 && event.angleDelta.y % 120 !== 0
                    const d = touchpad ? event.pixelDelta.y / 600 : event.angleDelta.y / 120 * 0.05
                    Player.volume = Math.max(0, Math.min(1, Player.volume + d))
                    volumeHint.opacity = 1
                    volumeHintHide.restart()
                }
            }

            RowLayout {
                anchors.fill: parent
                spacing: 12

                Item {
                    id: coverBox
                    Layout.preferredWidth: 48
                    Layout.preferredHeight: 48
                    MorphImage {
                        id: miniCover
                        anchors.fill: parent
                        visible: Player.cover !== ""
                        source: Player.cover
                        fallbackKind: Player.album !== "" ? "album" : "track"
                        fallbackArtist: Player.artist.split(", ")[0]
                        fallbackTitle: Player.album !== "" ? Player.album : Player.title
                        shape: "softSquare"
                        MaskSpin { target: miniCover; spinning: Settings.coverSpin && Player.playing && Player.isLocal === true }
                    }
                    MorphShape {
                        anchors.fill: parent
                        visible: Player.cover === ""
                        shape: "softSquare"
                        color: Theme.surfaceContainerHigh
                        Icon { anchors.centerIn: parent; name: Player.isEpisode ? "podcasts" : "music_note"; size: 24; color: Theme.fgSurfaceVariant }
                    }
                    // wheel volume: percentages over the cover, fading after a second
                    Rectangle {
                        id: volumeHint
                        anchors.fill: parent
                        radius: Theme.shape.medium
                        color: Qt.alpha(Theme.surfaceContainerHighest, 0.88)
                        opacity: 0
                        visible: opacity > 0
                        Behavior on opacity { NumberAnimation { duration: Theme.motion.effectsDefault } }
                        Label {
                            anchors.centerIn: parent
                            text: Math.round(Player.volume * 100) + "%"
                            type: "labelMedium"
                            weight: 600
                        }
                        Timer { id: volumeHintHide; interval: 1000; onTriggered: volumeHint.opacity = 0 }
                    }
                    // another device is playing — its badge in the cover corner
                    DeviceBadge {
                        anchors.right: parent.right
                        anchors.bottom: parent.bottom
                        anchors.rightMargin: -4
                        anchors.bottomMargin: -4
                        ringColor: Theme.surfaceContainerHighest
                    }
                }

                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 0
                    Label {
                        Layout.fillWidth: true
                        text: Player.hasTrack ? Player.title : I18n.t("Nothing playing")
                        type: "titleSmall"
                    }
                    // Artist · codec, or "Playing on …" (at the right edge — right above the time)
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 8
                        // Artists — each one is a link (card on hover). Episode, error, empty
                        // player and a tight pill with another device stay a single label
                        readonly property bool artistLinks: Player.errorText === "" && Player.hasTrack
                                                            && Player.isEpisode !== true
                                                            && !(remoteLabel.shown && !root.roomForMenu)
                        ArtistLinks {
                            Layout.fillWidth: true
                            visible: parent.artistLinks
                            // no hover card here: clicking the name opens the artist right away
                            preview: false
                            refs: Player.track ? (Player.track.artistRefs ?? []) : []
                            text: Player.artist
                            type: "bodySmall"
                            color: Theme.fgSurfaceVariant
                        }
                        LinkLabel {
                            Layout.fillWidth: true
                            visible: !parent.artistLinks && !(remoteLabel.shown && !root.roomForMenu && Player.errorText === "")
                            // for a podcast episode — a link to the podcast
                            linkEnabled: Player.errorText === "" && Player.hasTrack && Player.isEpisode === true
                                         && (Player.track.showId ?? "") !== ""
                            onClicked: Catalog.openShow(Player.track.showId ?? "")
                            text: Player.errorText !== "" ? Player.errorText
                                : Player.hasTrack ? Player.artist : I18n.t("Play a track, album or playlist")
                            type: "bodySmall"
                            color: Player.errorText !== "" ? Theme.error : Theme.fgSurfaceVariant
                        }
                        CodecBadge { Layout.alignment: Qt.AlignVCenter; Layout.bottomMargin: 3 }
                        RemoteLabel {
                            id: remoteLabel
                            Layout.alignment: Qt.AlignVCenter
                            Layout.maximumWidth: 220
                            brief: root.compact
                        }
                    }
                    RowLayout {
                        Layout.fillWidth: true
                        visible: Player.hasTrack
                        spacing: 8
                        WavyProgress {
                            id: seekBar
                            Layout.fillWidth: true
                            Layout.preferredHeight: 14
                            thickness: 3
                            amplitude: 2
                            wavelength: 28
                            value: root.progress
                            wavy: Player.playing
                            interactive: true
                            onCommitted: v => Player.seek(v)
                        }
                        Label {
                            text: root._time(seekBar.dragging ? seekBar.dragValue * Player.durationMs : Player.positionMs)
                                  + " / " + root._time(Player.durationMs)
                            type: "labelSmall"
                            color: Theme.fgSurfaceVariant
                        }
                    }
                }
            }

            TapHandler {
                enabled: Player.hasTrack
                onTapped: root.openNowPlaying(coverBox.mapToItem(null, coverBox.width / 2, coverBox.height / 2))
            }
            TapHandler {
                id: trackMenuTap
                enabled: Player.hasTrack
                acceptedButtons: Qt.RightButton
                onTapped: (point) => {
                    const p = trackMenuTap.parent.mapToItem(null, point.position.x, point.position.y)
                    Catalog.requestTrackMenu(Player.track, p.x, p.y)
                }
            }
        }

        // Rating: "do not recommend" — radio only (there it affects the selection and switches the track itself),
        // like — always, for an episode — "save" (bookmark)
        Fold { shown: Player.source === "radio" && !Player.isEpisode && !root.compact; IconButton {
            icon: "thumb_down"
            enabled: Player.hasTrack
            onClicked: Library.dislike(Player.track)
        } }
        LikeButton { visible: !Player.isEpisode; track: Player.track }
        IconButton {
            visible: Player.isEpisode
            icon: "bookmark"
            checkable: true
            autoToggle: false
            checked: Library.savedRevision >= 0 && Player.uri !== "" && Library.isSaved(Player.uri)
            enabled: Player.uri !== ""
            onClicked: Library.toggleSaved(Player.uri)
        }
        Fold { shown: root.roomForMenu || root.compact; IconButton {
            id: miniMore
            icon: "more_horiz"
            iconSize: 30
            iconWeight: 700
            iconColor: enabled ? Theme.fgSurface : Qt.alpha(Theme.fgSurface, Theme.stateLayer.disabledContent)
            enabled: Player.hasTrack
            onClicked: {
                const p = miniMore.mapToItem(null, 0, 0)
                Catalog.requestTrackMenuAt(Player.track, p.x, p.y, miniMore.width, miniMore.height)
            }
        } }
        Fold { shown: !root.compact; Item { width: 8; height: 8 } }

        // Panels and volume
        Fold { shown: !root.compact; IconButton {
            icon: "title"
            checkable: true
            autoToggle: false
            checked: Settings.rightPanelOpen && Settings.rightPanelTab === "lyrics"
            onClicked: root._togglePanel("lyrics")
        } }
        // The queue is also a drop target for tracks from lists: it highlights and grows slightly
        Fold { shown: !root.compact; IconButton {
            id: queueButton
            icon: "queue_music"
            checkable: true
            autoToggle: false
            checked: (Settings.rightPanelOpen && Settings.rightPanelTab === "queue") || queueDrop.containsDrag
            scale: queueDrop.containsDrag ? 1.08 : 1
            Behavior on scale { NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutBack } }
            onClicked: root._togglePanel("queue")
            DropArea {
                id: queueDrop
                anchors.fill: parent
                anchors.margins: -6
                keys: ["materialfy/track", "text/uri-list", "text/plain"]
                onDropped: drop => {
                    if (root._dropToQueue(drop))
                        drop.acceptProposedAction()
                }
            }
        } }
        // Spotify Connect devices (in the narrow pill too: transfer is needed there as well)
        Fold { DeviceButton { iconOnly: root.compact || !root.roomForDeviceName } }
        Fold { shown: root.roomForVolume && !root.compact; Row {
            id: volumeRow
            // volume of the playing device; on some devices it can only be changed on the device itself
            readonly property bool locked: Player.remote === true && Player.canSetVolume === false
            leftPadding: 8
            spacing: 4
            Icon {
                anchors.verticalCenter: parent.verticalCenter
                name: volumeRow.locked ? "lock" : Player.volume > 0.5 ? "volume_up" : Player.volume > 0 ? "volume_down" : "volume_off"
                size: 20
            }
            Slider {
                anchors.verticalCenter: parent.verticalCenter
                width: 96
                trackHeight: 10
                handleHeight: 28
                enabled: !volumeRow.locked
                opacity: enabled ? 1 : Theme.stateLayer.disabledContent
                value: Player.volume
                onMoved: v => Player.volume = v
            }
        } }
    }

    // Drop on the queue button: a track from a list (Drag.source with trackData() or track) or a Spotify link
    function _dropToQueue(drop) {
        const src = drop.source
        if (src) {
            const t = typeof src.trackData === "function" ? src.trackData() : src.track
            if (t && (t.uri || t.trackId)) {
                Player.addToQueue(t)
                return true
            }
        }
        const text = drop.hasUrls && drop.urls.length > 0 ? drop.urls[0].toString() : drop.hasText ? drop.text : ""
        const m = /(?:ytmusic:|music\.youtube\.com\/|youtube\.com\/watch\?v=)(track|episode|album|playlist)[:\/]([A-Za-z0-9_-]+)/.exec(text.trim())
        if (m) {
            Player.addUriToQueue("ytmusic:" + m[1] + ":" + m[2])
            return true
        }
        return false
    }

    function _togglePanel(tab) {
        if (Settings.rightPanelOpen && Settings.rightPanelTab === tab) {
            Settings.rightPanelOpen = false
        } else {
            Settings.rightPanelTab = tab
            Settings.rightPanelOpen = true
        }
    }
}
