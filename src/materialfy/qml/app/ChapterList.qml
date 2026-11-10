import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Episode chapters from timestamps in the description ("4:12 What bitrate is"). Click — seek if the episode
// is playing, otherwise start from that position. The current chapter is highlighted.
ColumnLayout {
    id: root

    property string text                // episode description
    property string uri                 // episode URI
    property int durationMs: 0

    readonly property bool isCurrent: uri !== "" && Player.uri === uri
    readonly property var chapters: parse(text, durationMs)
    readonly property int currentChapter: {
        if (!isCurrent)
            return -1
        let cur = -1
        for (let i = 0; i < chapters.length; i++)
            if (chapters[i].ms <= Player.positionMs)
                cur = i
        return cur
    }

    // a timestamp is at the start of a line or after a space/paren; the title runs to the next timestamp or end of line
    function parse(text, duration) {
        const re = /(^|[\s(\[])((?:\d{1,2}:)?\d{1,2}:\d{2})\)?\]?\s*[-–—:.|]?\s*/gm
        const found = []
        let m
        while ((m = re.exec(text ?? "")) !== null) {
            const parts = m[2].split(":").map(Number)
            const ms = (parts.length === 3 ? parts[0] * 3600 + parts[1] * 60 + parts[2] : parts[0] * 60 + parts[1]) * 1000
            found.push({ ms: ms, label: m[2], start: m.index + m[0].length, end: m.index + m[1].length })
        }
        const out = []
        for (let i = 0; i < found.length; i++) {
            const stop = i + 1 < found.length ? found[i + 1].end : (text ?? "").length
            let title = (text ?? "").slice(found[i].start, stop).split("\n")[0].trim().replace(/[\s,;·|–—([-]+$/, "")
            if (title.length > 90)
                title = title.slice(0, 88).trim() + "…"
            out.push({ ms: found[i].ms, label: found[i].label, title: title })
        }
        // chapters — only increasing timestamps inside the episode, at least two
        const ok = out.length >= 2 && out.every((c, i) => i === 0 || c.ms > out[i - 1].ms)
            && (duration <= 0 || out[out.length - 1].ms < duration)
        return ok ? out : []
    }

    visible: chapters.length > 0
    spacing: 2

    Label {
        Layout.bottomMargin: 6
        text: I18n.t("Chapters")
        type: "titleMedium"
        weight: 600
    }

    Repeater {
        model: root.chapters
        Item {
            id: chapterRow
            required property var modelData
            required property int index
            readonly property bool current: index === root.currentChapter
            Layout.fillWidth: true
            implicitHeight: 44

            Rectangle {
                anchors.fill: parent
                radius: Theme.shape.large
                color: chapterRow.current ? Qt.alpha(Theme.primary, 0.10) : "transparent"
                Behavior on color { enabled: !Theme.transitioning; ColorAnimation { duration: Theme.motion.effectsDefault } }
            }
            StateLayer {
                anchors.fill: parent
                radius: Theme.shape.large
                onClicked: {
                    if (root.isCurrent)
                        Player.seekMs(chapterRow.modelData.ms)
                    else
                        Player.playEpisode(root.uri, chapterRow.modelData.ms)
                }
            }
            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: 12
                anchors.rightMargin: 12
                spacing: 12
                Item {
                    Layout.preferredWidth: 8
                    Layout.preferredHeight: 8
                    Rectangle {
                        anchors.fill: parent
                        radius: 4
                        color: Theme.primary
                        visible: chapterRow.current
                    }
                }
                Label {
                    Layout.preferredWidth: 56
                    text: chapterRow.modelData.label
                    type: "labelLarge"
                    color: Theme.primary
                    font.features: ({ "tnum": 1 })
                }
                Label {
                    Layout.fillWidth: true
                    text: chapterRow.modelData.title
                    type: "bodyMedium"
                    color: chapterRow.current ? Theme.primary : Theme.fgSurface
                }
            }
        }
    }
}
