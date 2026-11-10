import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Episode progress card: play, "You stopped at …" / "Playing · left …", −10/+30 s,
// seek, "From the start", "To queue", "In your episodes", "Share".
Rectangle {
    id: root

    property var episode: ({})          // EPISODE_KEYS
    property bool compact: false
    signal shared()

    readonly property string uri: episode.uri ?? ""
    readonly property bool isCurrent: uri !== "" && Player.uri === uri
    readonly property int durationMs: isCurrent && Player.durationMs > 0 ? Player.durationMs : (episode.durationMs ?? 0)
    readonly property int resumeMs: episode.fullyPlayed === true ? 0 : (episode.resumeMs ?? 0)
    readonly property int positionMs: isCurrent ? (seek.dragging ? seek.dragValue * durationMs : Player.positionMs) : resumeMs
    readonly property bool saved: Library.savedRevision >= 0 && uri !== "" ? Library.isSaved(uri) : episode.saved === true

    function _clock(ms) {
        const s = Math.max(0, Math.round(ms / 1000))
        const h = Math.floor(s / 3600)
        const mm = Math.floor(s / 60) % 60
        const ss = String(s % 60).padStart(2, "0")
        return h > 0 ? h + ":" + String(mm).padStart(2, "0") + ":" + ss : mm + ":" + ss
    }
    function _minutes(ms) {
        return I18n.duration(Math.max(60000, ms))
    }
    function play() {
        if (isCurrent)
            Player.togglePlay()
        else
            Player.playEpisode(uri, resumeMs)
    }
    function playFrom(ms) {
        if (isCurrent)
            Player.seekMs(ms)
        else
            Player.playEpisode(uri, ms)
    }

    implicitHeight: body.implicitHeight + 40
    implicitWidth: 560
    radius: Theme.shape.extraLarge
    color: Theme.surfaceContainerHigh

    ColumnLayout {
        id: body
        x: 20
        y: 20
        width: root.width - 40
        spacing: 16

        RowLayout {
            Layout.fillWidth: true
            spacing: 16

            PlayButton {
                size: 64
                playing: root.isCurrent && Player.playing
                pausedShape: "circle"
                spin: false
                onClicked: root.play()
            }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 2
                Label {
                    Layout.fillWidth: true
                    text: root.isCurrent ? (Player.playing ? I18n.t("Playing · {0} left", root._minutes(root.durationMs - root.positionMs))
                                                          : I18n.t("Paused · {0} left", root._minutes(root.durationMs - root.positionMs)))
                        : episode.fullyPlayed === true ? I18n.t("Played")
                        : root.resumeMs > 0 ? I18n.t("You stopped at {0}", root._clock(root.resumeMs))
                        : I18n.t("Not played yet")
                    type: "titleMedium"
                    color: root.isCurrent ? Theme.primary : Theme.fgSurface
                }
                Label {
                    Layout.fillWidth: true
                    text: root.durationMs > 0 ? I18n.t("Duration {0}", root._minutes(root.durationMs)) : ""
                    visible: text !== ""
                    type: "bodySmall"
                    color: Theme.fgSurfaceVariant
                }
            }
            IconButton {
                icon: "replay_10"
                enabled: root.isCurrent
                onClicked: Player.seekRelative(-10000)
            }
            IconButton {
                icon: "forward_30"
                enabled: root.isCurrent
                onClicked: Player.seekRelative(30000)
            }
        }

        ColumnLayout {
            Layout.fillWidth: true
            spacing: 4
            WavyProgress {
                id: seek
                Layout.fillWidth: true
                Layout.preferredHeight: 20
                value: root.durationMs > 0 ? root.positionMs / root.durationMs : 0
                wavy: root.isCurrent && Player.playing && !Theme.quiet && !Theme.reducedMotion
                interactive: root.durationMs > 0
                onCommitted: v => root.playFrom(Math.round(v * root.durationMs))
            }
            RowLayout {
                Layout.fillWidth: true
                Label { text: root._clock(root.positionMs); type: "labelMedium"; color: Theme.fgSurfaceVariant }
                Item { Layout.fillWidth: true }
                Label { text: "−" + root._clock(root.durationMs - root.positionMs); type: "labelMedium"; color: Theme.fgSurfaceVariant }
            }
        }

        Flow {
            Layout.fillWidth: true
            spacing: 8
            Button {
                text: I18n.t("From the start")
                icon: "restart_alt"
                style: "tonal"
                size: "xs"
                onClicked: root.playFrom(0)
            }
            Button {
                text: I18n.t("Queue it")
                icon: "add_to_queue"
                style: "outlined"
                size: "xs"
                onClicked: Player.addUriToQueue(root.uri)
            }
            Button {
                text: root.saved ? I18n.t("In Your Episodes") : I18n.t("Save to Your Episodes")
                icon: root.saved ? "bookmark_added" : "bookmark"
                style: "outlined"
                size: "xs"
                checkable: true
                checked: root.saved
                autoToggle: false
                onClicked: Library.toggleSaved(root.uri)
            }
            Button {
                text: root.compact ? "" : I18n.t("Share")
                icon: "share"
                style: "outlined"
                size: "xs"
                onClicked: {
                    Catalog.copyLink(Catalog.uriUrl(root.uri))
                    root.shared()
                }
            }
        }
    }
}
