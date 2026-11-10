import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// The "Radio" scene on the home page. On the left (top in a narrow window) is the station card: the shape of the selected seed's group
// with its cover and a button, a label and actions. Next to it — seed groups in even rows: "Recent tracks",
// "Artists", "Albums and playlists", "Any track" with a "+". Each seed has its own cell: the label is elided
// inside it and never overlaps anything; the group title is a separate row above the line.
// Seeds used to sit on an elliptical orbit around the center — labels ran into each other and over the cover.
// Group models are refreshed in place (by URI): changing the seeds or the selection does not recreate the other cells.
Item {
    id: root

    property var groups: []                // Radio.seeds: [{key, title, items: [{uri, label, cover, kind}]}]
    property string selected               // URI of the selected seed
    property bool locked: false            // no playback on this PC — the seeds are inactive
    property bool loading: false
    property bool playing: false           // the selected seed's station is playing
    property bool signedIn: true
    property bool premium: true
    property string errorText
    property bool compact: false

    signal picked(string uri)
    signal addRequested(Item seed)
    signal playClicked()
    signal openList()
    signal saveRequested()
    signal refreshRequested()
    signal connectRequested()
    signal moreRequested()
    signal loginRequested()

    readonly property bool reduced: Theme.reducedMotion === true
    // The card sits left of the groups if at least 6 cells fit beside it; otherwise above them
    readonly property real heroWidth: Math.max(300, Math.min(400, width * 0.32))
    readonly property bool sideBySide: signedIn && width - heroWidth - 24 >= 6 * 88
    // A seed cell stretches to the width: the longest group (7 with "+") is a single line, longer labels
    readonly property real groupsWidth: sideBySide ? width - heroWidth - 24 : width
    readonly property real slotWidth: Math.floor(Math.max(compact ? 76 : 84, Math.min(112, groupsWidth / 7 - 4)))
    readonly property real seedSize: Math.max(48, Math.min(64, Math.round(slotWidth * 0.62)))

    implicitHeight: column.implicitHeight

    // Tonal pairs for groups: color / container / text on them; the group shape
    readonly property var styles: ({
        recent:      { role: "primary", shape: "softSquare" },
        artists:     { role: "tertiary", shape: "softSquare" },
        collections: { role: "secondary", shape: "softSquare" },
        any:         { role: "neutral", shape: "softSquare" }
    })
    readonly property var groupKeys: ["recent", "artists", "collections", "any"]
    readonly property var defaultTitles: ({ recent: I18n.t("Recent tracks"), artists: I18n.t("Your artists"), collections: I18n.t("Albums & playlists"), any: I18n.t("Any track") })
    function roleColor(role) { return role === "neutral" ? Theme.fgSurfaceVariant : Theme[role] }
    function roleBox(role) { return role === "neutral" ? Theme.surfaceVariant : Theme[role + "Container"] }

    function groupOf(key) {
        for (const g of (signedIn ? groups ?? [] : []))
            if (g && g.key === key)
                return g
        return null
    }

    readonly property var current: {
        if (!signedIn || selected === "")
            return null
        for (const g of (groups ?? [])) {
            const items = g && g.items ? g.items : []
            for (let j = 0; j < items.length; j++)
                if (items[j].uri === selected)
                    return { group: g, item: items[j], first: j === 0 }
        }
        return null
    }
    readonly property var currentStyle: current ? (styles[current.group.key] ?? styles.any) : styles.recent

    // Seed list → ListModel in place: matching cells stay, shifted ones move (move),
    // new ones are inserted — without recreating the whole row
    function syncModel(model, items) {
        for (let i = 0; i < items.length; ++i) {
            const it = { uri: items[i].uri ?? "", label: items[i].label ?? "", cover: items[i].cover ?? "", kind: items[i].kind ?? "" }
            let j = i
            while (j < model.count && model.get(j).uri !== it.uri)
                j++
            if (j >= model.count) {
                model.insert(i, it)
                continue
            }
            if (j !== i)
                model.move(j, i, 1)
            const o = model.get(i)
            if (o.label !== it.label || o.cover !== it.cover || o.kind !== it.kind)
                model.set(i, it)
        }
        if (model.count > items.length)
            model.remove(items.length, model.count - items.length)
    }

    ColumnLayout {
        id: column
        width: root.width
        spacing: 16

        Label {
            visible: root.signedIn
            text: I18n.t("Radio")
            type: "headlineSmall"
            weight: 600
        }

        GridLayout {
            Layout.fillWidth: true
            columns: root.sideBySide ? 2 : 1
            columnSpacing: 24
            rowSpacing: 20

            // ---- Station card ----
            Rectangle {
                id: card
                Layout.preferredWidth: root.sideBySide ? root.heroWidth : -1
                Layout.fillWidth: !root.sideBySide
                Layout.fillHeight: root.sideBySide
                Layout.alignment: Qt.AlignTop
                implicitHeight: heroGrid.implicitHeight + 2 * pad
                radius: Theme.shape.extraLarge
                color: Theme.surfaceContainerHigh
                readonly property real pad: root.sideBySide ? 24 : 20
                readonly property bool closed: root.locked || !root.signedIn

                GridLayout {
                    id: heroGrid
                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.verticalCenter: parent.verticalCenter
                    anchors.margins: card.pad
                    columns: root.sideBySide ? 1 : 2
                    columnSpacing: 24
                    rowSpacing: 16

                    // the group shape, with the seed cover and "Play" inside
                    Item {
                        id: hero
                        readonly property real size: root.sideBySide ? Math.min(220, card.width - 2 * card.pad)
                                                                     : (root.compact ? 136 : 160)
                        Layout.preferredWidth: size
                        Layout.preferredHeight: size
                        Layout.alignment: Qt.AlignHCenter | Qt.AlignVCenter

                        MorphShape {
                            anchors.fill: parent
                            shape: card.closed ? "softSquare" : root.currentStyle.shape
                            duration: root.reduced ? 0 : Theme.motion.spatialSlow
                            color: card.closed ? Theme.surfaceContainerHighest : root.roleBox(root.currentStyle.role)
                            Behavior on color { enabled: !Theme.transitioning && !root.reduced; ColorAnimation { duration: Theme.motion.effectsSlow } }
                        }
                        MorphImage {
                            width: parent.width * 0.7
                            height: width
                            anchors.centerIn: parent
                            visible: !card.closed && root.current !== null && (root.current.item.cover ?? "") !== ""
                            source: visible ? root.current.item.cover : ""
                            shape: "softSquare"
                            duration: root.reduced ? 0 : Theme.motion.spatialSlow
                        }
                        PlayButton {
                            anchors.centerIn: parent
                            visible: !card.closed && !root.loading
                            size: Math.round(hero.size * 0.32)
                            playing: root.playing
                            spin: false
                            pausedShape: "softSquare"
                            playingShape: "softSquare"
                            enabled: root.current !== null
                            onClicked: root.playClicked()
                        }
                        LoadingIndicator {
                            anchors.centerIn: parent
                            visible: root.loading && !card.closed
                            size: Math.round(hero.size * 0.38)
                            color: Theme.primary
                        }
                        Icon {
                            anchors.centerIn: parent
                            visible: card.closed
                            name: "lock"
                            size: 32
                            color: Theme.fgSurfaceVariant
                        }
                    }

                    // label: which station it is, and the actions
                    ColumnLayout {
                        id: caption
                        Layout.fillWidth: true
                        // width from the card, not from the content: a long row of buttons does not push the label past the edge
                        Layout.maximumWidth: room
                        Layout.alignment: Qt.AlignVCenter
                        spacing: 4
                        readonly property int align: root.sideBySide ? Text.AlignHCenter : Text.AlignLeft
                        readonly property real room: Math.max(0, heroGrid.width - (root.sideBySide ? 0 : hero.size + heroGrid.columnSpacing))

                        readonly property string kindText: {
                            const c = root.current
                            if (!c)
                                return I18n.t("Radio")
                            return ({ track: I18n.t("Track radio"), artist: I18n.t("Artist radio"), album: I18n.t("Album radio"), playlist: I18n.t("Playlist radio") })[c.item.kind] ?? I18n.t("Radio")
                        }

                        Label {
                            Layout.fillWidth: true
                            horizontalAlignment: caption.align
                            text: !root.signedIn || root.locked ? I18n.t("Radio unavailable")
                                : root.loading ? I18n.t("Radio")
                                : (root.playing ? I18n.t("Now playing") + " · " : "") + caption.kindText
                                  + (root.current && root.current.first && root.current.group.key === "recent" ? " · " + I18n.t("latest") : "")
                            type: "labelLarge"
                            wrapMode: Text.WordWrap
                            maximumLineCount: 2
                            color: card.closed ? Theme.fgSurfaceVariant : root.roleColor(root.currentStyle.role)
                            font.capitalization: Font.AllUppercase
                            font.letterSpacing: 1.2
                        }
                        Label {
                            Layout.fillWidth: true
                            horizontalAlignment: caption.align
                            text: !root.signedIn ? I18n.t("Sign in to YouTube Music")
                                : root.locked ? (root.premium ? I18n.t("Connect playback on this computer") : I18n.t("Stations need a signed-in account"))
                                : root.loading ? I18n.t("Building your station…")
                                : root.current ? root.current.item.label ?? "" : I18n.t("Pick a track, artist or playlist")
                            type: root.locked || !root.signedIn || root.loading || !root.current ? "titleLarge" : "headlineSmall"
                            weight: 500
                            wrapMode: Text.WordWrap
                            maximumLineCount: 3
                        }
                        Label {
                            Layout.fillWidth: true
                            horizontalAlignment: caption.align
                            text: !root.signedIn ? I18n.t("Stations based on your tracks, artists and playlists will appear after you sign in.")
                                : root.locked ? (root.premium
                                    ? I18n.t("Stations are available on this computer once you sign in.")
                                    : I18n.t("Stations are available on this computer once you sign in."))
                                : root.errorText !== "" ? root.errorText
                                : root.loading ? I18n.t("50 tracks in one batch")
                                : I18n.t("50 tracks · a fresh mix every time")
                            type: "bodySmall"
                            color: root.errorText !== "" && !root.locked && root.signedIn ? Theme.error : Theme.fgSurfaceVariant
                            wrapMode: Text.WordWrap
                            maximumLineCount: 4
                        }
                        Grid {
                            id: actions
                            Layout.topMargin: 8
                            Layout.alignment: root.sideBySide ? Qt.AlignHCenter : Qt.AlignLeft
                            visible: !root.loading
                            spacing: 8
                            // they do not fit on the card's line — a centered column
                            columns: rowWidth > caption.room ? 1 : 8
                            horizontalItemAlignment: Grid.AlignHCenter
                            verticalItemAlignment: Grid.AlignVCenter
                            readonly property real rowWidth: {
                                let w = 0, n = 0
                                for (const c of children)
                                    if (c.visible) { w += c.implicitWidth; n++ }
                                return w + Math.max(0, n - 1) * spacing
                            }
                            // sign in / connect playback
                            Button {
                                visible: !root.signedIn
                                text: I18n.t("Sign in")
                                icon: "login"
                                onClicked: root.loginRequested()
                            }
                            Button {
                                visible: root.signedIn && root.locked
                                text: I18n.t("Connect playback")
                                icon: "login"
                                onClicked: root.connectRequested()
                            }
                            Button {
                                visible: root.signedIn && root.locked && root.premium
                                text: I18n.t("Learn more")
                                style: "text"
                                onClicked: root.moreRequested()
                            }
                            // station
                            Button {
                                visible: root.signedIn && !root.locked && root.current !== null
                                text: I18n.t("Track list")
                                icon: "format_list_bulleted"
                                style: "tonal"
                                size: "xs"
                                onClicked: root.openList()
                            }
                            IconButton {
                                visible: root.signedIn && !root.locked && root.current !== null
                                icon: "bookmark_add"
                                size: "xs"
                                onClicked: root.saveRequested()
                            }
                            IconButton {
                                visible: root.signedIn && !root.locked && root.current !== null
                                icon: "refresh"
                                size: "xs"
                                onClicked: root.refreshRequested()
                            }
                        }
                    }
                }
            }

            // ---- Seed groups ----
            ColumnLayout {
                visible: root.signedIn
                Layout.fillWidth: true
                Layout.alignment: Qt.AlignTop
                spacing: 12
                opacity: root.locked ? 0.38 : 1
                enabled: !root.locked && root.signedIn

                Repeater {
                    model: root.groupKeys          // a fixed list: the groups are not recreated
                    ColumnLayout {
                        id: group
                        required property string modelData
                        readonly property string key: modelData
                        readonly property var info: root.groupOf(key)
                        readonly property var style: root.styles[key] ?? root.styles.any
                        Layout.fillWidth: true
                        visible: key === "any" || seeds.count > 0
                        spacing: 4

                        ListModel { id: seeds }
                        function sync() {
                            const items = (info && info.items ? info.items : []).slice()
                            if (key === "any")
                                items.push({ uri: "", label: I18n.t("Find a track"), cover: "", kind: "add" })
                            root.syncModel(seeds, items)
                        }
                        onInfoChanged: sync()
                        Component.onCompleted: sync()

                        // group title: a shape icon in the group color and the name
                        RowLayout {
                            Layout.fillWidth: true
                            spacing: 8
                            MorphShape {
                                Layout.preferredWidth: 14
                                Layout.preferredHeight: 14
                                shape: group.style.shape
                                color: root.roleColor(group.style.role)
                            }
                            Label {
                                Layout.fillWidth: true
                                text: group.info && group.info.title ? group.info.title : root.defaultTitles[group.key]
                                type: "titleSmall"
                                weight: 600
                                color: root.roleColor(group.style.role)
                            }
                        }

                        Flow {
                            Layout.fillWidth: true
                            Layout.leftMargin: -(root.slotWidth - root.seedSize) / 2 + 2  // circles along the title edge
                            spacing: 4
                            Repeater {
                                model: seeds
                                Item {
                                    id: seed
                                    required property string uri
                                    required property string label
                                    required property string cover
                                    required property string kind
                                    readonly property bool add: kind === "add"
                                    readonly property bool on: !add && uri !== "" && uri === root.selected
                                    readonly property bool hovered: area.containsMouse
                                    width: root.slotWidth
                                    height: root.seedSize + 34

                                    Item {
                                        id: disc
                                        width: root.seedSize
                                        height: root.seedSize
                                        y: 6
                                        anchors.horizontalCenter: parent.horizontalCenter

                                        // a halo in the group color on the selected one
                                        MorphShape {
                                            anchors.centerIn: parent
                                            width: parent.width + 10
                                            height: width
                                            visible: opacity > 0
                                            shape: group.style.shape
                                            color: root.roleBox(group.style.role)
                                            scale: seed.on ? 1 : 0.7
                                            opacity: seed.on ? 1 : 0
                                            Behavior on scale { enabled: !root.reduced; NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.OutBack } }
                                            Behavior on opacity { NumberAnimation { duration: root.reduced ? 0 : Theme.motion.effectsDefault } }
                                        }
                                        MorphImage {
                                            anchors.fill: parent
                                            visible: !seed.add && seed.cover !== ""
                                            source: visible ? seed.cover : ""
                                            shape: seed.on ? group.style.shape : seed.hovered ? "softSquare" : "circle"
                                            duration: root.reduced ? 0 : Theme.motion.spatialDefault
                                        }
                                        MorphShape {
                                            anchors.fill: parent
                                            visible: seed.add || seed.cover === ""
                                            shape: seed.on ? group.style.shape : seed.hovered ? "softSquare" : "circle"
                                            duration: root.reduced ? 0 : Theme.motion.spatialDefault
                                            color: seed.add ? Theme.surfaceVariant : root.roleBox(group.style.role)
                                            Icon {
                                                anchors.centerIn: parent
                                                name: seed.add ? "add" : ({ track: "music_note", artist: "person", album: "album", playlist: "queue_music" })[seed.kind] ?? "radio"
                                                size: root.seedSize * 0.42
                                                color: Theme.fgSurfaceVariant
                                            }
                                        }
                                    }
                                    // label — within its own cell
                                    Label {
                                        anchors.top: disc.bottom
                                        anchors.topMargin: 6
                                        x: 2
                                        width: parent.width - 4
                                        fade: true
                                        horizontalAlignment: overflowing ? Text.AlignLeft : Text.AlignHCenter
                                        text: seed.label
                                        type: "labelMedium"
                                        weight: seed.on ? 700 : 500
                                        color: seed.on ? root.roleColor(group.style.role)
                                             : seed.hovered ? Theme.fgSurface : Theme.fgSurfaceVariant
                                    }
                                    MouseArea {
                                        id: area
                                        anchors.fill: parent
                                        hoverEnabled: true
                                        cursorShape: Qt.PointingHandCursor
                                        onClicked: seed.add ? root.addRequested(disc) : root.picked(seed.uri)
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
