import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// MD3 kit gallery: all components + theme control. An image can be dragged into the window — the theme takes its color from it.
Window {
    id: win

    // demo cover — the logo from docs/brand
    readonly property url coversDir: Qt.resolvedUrl("../../../../docs/brand/")
    readonly property var covers: ["icon-512.png"]
    property int coverIndex: 0
    property bool playing: true
    property real progress: 0.32
    property real scrollTo: 0
    property int startCover: 0
    property bool perfLog: false        // --set perfLog=true: cycle the covers and log frame times
    property bool perfScroll: false     // --set perfScroll=true: scroll up and down instead of the covers

    Timer {
        interval: 1600
        repeat: true
        running: win.perfLog
        onTriggered: {
            if (perf.frames > 0)
                console.log(`perf: frames=${perf.frames} avg=${(perf.total / perf.frames).toFixed(1)}ms max=${perf.max.toFixed(1)}ms slow(>20ms)=${perf.slow}`)
            perf.frames = 0; perf.total = 0; perf.max = 0; perf.slow = 0
            if (win.perfScroll)
                flick.scrollTo(flick.contentY > flick.maxY / 2 ? 0 : flick.maxY)
            else
                win.useCover((win.coverIndex + 1) % win.covers.length)
        }
    }

    FrameAnimation {
        id: perf
        property int frames: 0
        property real total: 0
        property real max: 0
        property int slow: 0
        running: win.perfLog
        onTriggered: {
            const ms = frameTime * 1000
            frames++; total += ms; max = Math.max(max, ms); if (ms > 20) slow++
        }
    }

    width: 1280
    height: 800
    visible: true
    title: "Materialfy — MD3 Gallery"
    flags: Qt.Window | Qt.FramelessWindowHint
    color: Theme.surfaceContainerLowest

    function useCover(i) {
        coverIndex = i
        ThemeEngine.setSeedFromImage(coversDir + covers[i])
    }

    Component.onCompleted: {
        useCover(startCover)
        Qt.callLater(() => flick.contentY = Math.min(scrollTo, Math.max(0, flick.contentHeight - flick.height)))
    }

    Timer {
        interval: 100
        repeat: true
        running: win.playing && !seek.dragging
        onTriggered: win.progress = (win.progress + 0.002) % 1
    }

    DropArea {
        anchors.fill: parent
        onDropped: drop => { if (drop.hasUrls) ThemeEngine.setSeedFromImage(drop.urls[0]) }
    }

    RowLayout {
        anchors.fill: parent
        anchors.margins: 8
        spacing: 8

        // ---------------- Sidebar ----------------
        ColumnLayout {
            Layout.preferredWidth: 260
            Layout.fillWidth: false
            Layout.fillHeight: true
            spacing: 2

            Item { Layout.preferredHeight: 12 }
            NavItem { Layout.fillWidth: true; icon: "home"; text: I18n.t("Home") }
            NavItem { Layout.fillWidth: true; icon: "search"; text: I18n.t("Search"); selected: true }

            SectionHeader { text: I18n.t("Collection") }
            NavItem { Layout.fillWidth: true; icon: "favorite"; text: I18n.t("My likes") }
            NavItem { Layout.fillWidth: true; icon: "graphic_eq"; text: I18n.t("My Wave") }

            SectionHeader { text: I18n.t("Playlists"); action: "add" }
            Repeater {
                model: win.covers
                NavItem {
                    required property string modelData
                    required property int index
                    Layout.fillWidth: true
                    text: I18n.t("Playlist {0}", index + 1)
                    imageSource: win.coversDir + modelData
                    selected: win.coverIndex === index
                    trailingIcon: "download_done"
                    onClicked: win.useCover(index)
                }
            }
            Item { Layout.fillHeight: true }
        }

        // ---------------- Content ----------------
        Surface {
            Layout.fillWidth: true
            Layout.fillHeight: true
            level: "container"
            clip: true

            SmoothFlickable {
                id: flick
                anchors.fill: parent
                contentHeight: col.implicitHeight + 48

                ColumnLayout {
                    id: col
                    x: 24
                    y: 24
                    width: flick.width - 48
                    spacing: 28

                    RowLayout {
                        Layout.fillWidth: true
                        IconButton { icon: "arrow_back" }
                        Item { Layout.fillWidth: true }
                        SearchField { Layout.preferredWidth: 380; placeholder: I18n.t("Track, album, artist"); text: "death metal" }
                        Item { Layout.fillWidth: true }
                        IconButton { icon: "more_vert" }
                    }

                    Label { text: "MD3 Expressive"; type: "displaySmall"; weight: 600 }

                    // --- Input fields ---
                    Flow {
                        Layout.fillWidth: true
                        spacing: 16
                        TextField { width: 260; label: I18n.t("Host"); placeholderText: "proxy.example.com"; leadingIcon: "lan" }
                        TextField { width: 200; label: I18n.t("Password"); echoMode: TextInput.Password; text: "secret" }
                        MenuField {
                            id: galleryMenuField
                            width: 240
                            label: I18n.t("Output device")
                            leadingIcon: "speaker"
                            model: [{ id: "", text: I18n.t("System default") }, { id: "spk", text: I18n.t("Speakers") }, { id: "hp", text: I18n.t("Headphones") }]
                            currentValue: ""
                            onActivated: v => currentValue = v
                        }
                    }

                    // --- Player ---
                    Surface {
                        Layout.fillWidth: true
                        Layout.preferredHeight: 220
                        level: "high"

                        RowLayout {
                            anchors.fill: parent
                            anchors.margins: 24
                            spacing: 28

                            MorphImage {
                                id: bigCover
                                Layout.preferredWidth: 148
                                Layout.preferredHeight: 148
                                source: win.coversDir + win.covers[win.coverIndex]
                                shape: win.playing ? "cookie12" : "square"
                                duration: Theme.motion.spatialSlow
                            }

                            ColumnLayout {
                                Layout.fillWidth: true
                                spacing: 6
                                Label { text: "Materialfy"; type: "headlineSmall"; weight: 700; Layout.fillWidth: true }
                                Label { text: "Material 3 Expressive"; type: "bodyLarge"; color: Theme.fgSurfaceVariant }

                                RowLayout {
                                    Layout.fillWidth: true
                                    Layout.topMargin: 10
                                    spacing: 12
                                    Label { text: "1:46"; type: "labelMedium"; color: Theme.fgSurfaceVariant }
                                    WavyProgress {
                                        id: seek
                                        Layout.fillWidth: true
                                        interactive: true
                                        value: win.progress
                                        wavy: win.playing
                                        onCommitted: v => win.progress = v
                                    }
                                    Label { text: "-2:35"; type: "labelMedium"; color: Theme.fgSurfaceVariant }
                                }

                                RowLayout {
                                    Layout.fillWidth: true
                                    spacing: 8
                                    IconButton { icon: "shuffle"; checkable: true; checked: true; style: "tonal"; size: "s" }
                                    IconButton { icon: "skip_previous"; iconColor: Theme.fgSurface }
                                    PlayButton { size: 64; playing: win.playing; onClicked: win.playing = !win.playing }
                                    IconButton { icon: "skip_next"; iconColor: Theme.fgSurface }
                                    IconButton { icon: "repeat"; checkable: true; size: "s" }
                                    Icon { name: "volume_up"; size: 20; Layout.leftMargin: 8 }
                                    Slider { Layout.fillWidth: true; Layout.minimumWidth: 80; value: 0.7; trackHeight: 12; handleHeight: 32 }
                                }
                            }
                        }
                    }

                    // --- Buttons ---
                    Section { title: I18n.t("Buttons") }
                    Flow {
                        Layout.fillWidth: true
                        spacing: 12
                        Button { text: "Filled"; icon: "play_arrow" }
                        Button { text: "Tonal"; style: "tonal"; icon: "shuffle" }
                        Button { text: "Outlined"; style: "outlined" }
                        Button { text: "Elevated"; style: "elevated"; icon: "add" }
                        Button { text: "Text"; style: "text" }
                        Button { text: "Disabled"; enabled: false }
                        Button { text: "Toggle"; style: "tonal"; checkable: true; icon: "favorite" }
                    }
                    Flow {
                        Layout.fillWidth: true
                        spacing: 12
                        Button { text: "XS"; size: "xs" }
                        Button { text: "Small"; size: "s" }
                        Button { text: "Medium"; size: "m"; icon: "graphic_eq" }
                        Button { text: I18n.t("My Wave"); size: "l"; icon: "graphic_eq"; style: "tonal" }
                    }

                    Section { title: I18n.t("Icon buttons") }
                    Flow {
                        Layout.fillWidth: true
                        spacing: 12
                        IconButton { icon: "favorite"; checkable: true }
                        IconButton { icon: "favorite"; checkable: true; style: "filled" }
                        IconButton { icon: "title"; checkable: true; checked: true; style: "tonal" }
                        IconButton { icon: "queue_music"; style: "outlined" }
                        IconButton { icon: "bedtime"; checkable: true; style: "outlined" }
                        IconButton { icon: "skip_next"; style: "filled"; size: "m"; widthMode: "wide" }
                        IconButton { icon: "play_arrow"; style: "filled"; size: "l" }
                        IconButton { icon: "graphic_eq"; style: "tonal"; size: "l"; widthMode: "narrow" }
                    }

                    Section { title: I18n.t("Button group") }
                    Flow {
                        Layout.fillWidth: true
                        spacing: 24
                        ButtonGroup { model: ["Normal", "Circular"] }
                        ButtonGroup { model: ["Bars", "Wave"]; currentIndex: 1; showCheck: true }
                        ButtonGroup {
                            model: [{ icon: "photo" }, { icon: "title" }, { icon: "queue_music" }, { icon: "bedtime" }, { icon: "tune" }]
                            currentIndex: 1
                        }
                    }

                    Section { title: I18n.t("Shapes (click to morph)") }
                    Flow {
                        Layout.fillWidth: true
                        spacing: 16
                        Repeater {
                            model: ["circle", "square", "softSquare", "cookie4", "cookie6", "cookie9", "cookie12", "scallop", "clover4", "flower6", "flower8", "sunny", "blob", "puffy"]
                            Column {
                                id: cell
                                required property string modelData
                                required property int index
                                spacing: 6
                                MorphShape {
                                    id: ms
                                    width: 72
                                    height: 72
                                    shape: cell.modelData
                                    color: [Theme.primary, Theme.secondary, Theme.tertiary, Theme.primaryContainer][cell.index % 4]
                                    MouseArea {
                                        anchors.fill: parent
                                        cursorShape: Qt.PointingHandCursor
                                        onClicked: ms.shape = ms.shape === cell.modelData ? "circle" : cell.modelData
                                    }
                                }
                                Label { text: cell.modelData; type: "labelSmall"; color: Theme.fgSurfaceVariant; anchors.horizontalCenter: parent.horizontalCenter }
                            }
                        }
                    }

                    Section { title: I18n.t("Fields and switches") }
                    RowLayout {
                        spacing: 24
                        Switch { checked: true }
                        Switch { checked: false }
                        Switch { checked: false; icons: false }
                        Slider { Layout.preferredWidth: 280; value: 0.6; from: 0; to: 2; valueText: v => Math.round(v * 100) + "%" }
                    }

                    Section { title: I18n.t("Color roles") }
                    Flow {
                        Layout.fillWidth: true
                        spacing: 8
                        Repeater {
                            model: ["primary", "fgPrimary", "primaryContainer", "fgPrimaryContainer", "secondary", "secondaryContainer", "tertiary", "tertiaryContainer",
                                    "surface", "surfaceContainerLow", "surfaceContainer", "surfaceContainerHigh", "surfaceContainerHighest", "fgSurface", "fgSurfaceVariant", "outline", "outlineVariant", "error"]
                            Rectangle {
                                required property string modelData
                                width: 132
                                height: 56
                                radius: Theme.shape.medium
                                color: Theme[modelData]
                                border.width: 1
                                border.color: Theme.outlineVariant
                                Label {
                                    anchors.fill: parent
                                    anchors.margins: 8
                                    verticalAlignment: Text.AlignBottom
                                    text: parent.modelData
                                    type: "labelSmall"
                                    color: Qt.colorEqual(parent.color, "transparent") ? Theme.fgSurface : (parent.color.hslLightness > 0.5 ? "black" : "white")
                                }
                            }
                        }
                    }
                    Item { Layout.preferredHeight: 24 }
                }
            }
        }

        // ---------------- Theme panel ----------------
        Surface {
            Layout.preferredWidth: 300
            Layout.fillHeight: true
            level: "container"

            ColumnLayout {
                anchors.fill: parent
                anchors.margins: 24
                spacing: 16

                Label { text: I18n.t("Theme"); type: "titleLarge" }

                RowLayout {
                    Layout.fillWidth: true
                    Label { text: I18n.t("Dark"); type: "bodyLarge"; Layout.fillWidth: true }
                    Switch { checked: ThemeEngine.dark; onToggled: c => ThemeEngine.dark = c }
                }

                Label { text: I18n.t("Color from cover"); type: "labelLarge"; color: Theme.fgSurfaceVariant }
                Row {
                    spacing: 12
                    Repeater {
                        model: win.covers
                        MorphImage {
                            required property string modelData
                            required property int index
                            width: 72
                            height: 72
                            source: win.coversDir + modelData
                            shape: win.coverIndex === index ? "cookie9" : "softSquare"
                            MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: win.useCover(parent.index) }
                        }
                    }
                }

                Label { text: I18n.t("Or a seed color"); type: "labelLarge"; color: Theme.fgSurfaceVariant }
                Flow {
                    Layout.fillWidth: true
                    spacing: 8
                    Repeater {
                        model: ["#FFCC00", String(Theme.defaultSeed).toUpperCase(), "#6750A4", "#E8175D", "#00A3FF", "#FF6D00"]
                        MorphShape {
                            required property string modelData
                            width: 32
                            height: 32
                            shape: Qt.colorEqual(ThemeEngine.seed, modelData) ? "cookie6" : "circle"
                            color: modelData
                            MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: ThemeEngine.setSeed(parent.modelData) }
                        }
                    }
                }

                Label { text: I18n.t("Scheme"); type: "labelLarge"; color: Theme.fgSurfaceVariant }
                Flow {
                    Layout.fillWidth: true
                    spacing: 8
                    Repeater {
                        model: ThemeEngine.variants
                        Chip {
                            required property string modelData
                            text: modelData
                            selected: ThemeEngine.variant === modelData
                            onClicked: ThemeEngine.variant = modelData
                        }
                    }
                }

                Item { Layout.fillHeight: true }
                Label {
                    Layout.fillWidth: true
                    text: I18n.t("Drop any image onto the window — the theme will take its color.")
                    type: "bodySmall"
                    wrapMode: Text.WordWrap
                    elide: Text.ElideNone
                    lineHeightMode: Text.ProportionalHeight
                    lineHeight: 1.2
                    color: Theme.fgSurfaceVariant
                }
            }
        }
    }

    component Section: Label {
        property string title
        text: title
        type: "titleMedium"
        color: Theme.primary
    }

    component SectionHeader: RowLayout {
        property alias text: lbl.text
        property string action
        Layout.fillWidth: true
        Layout.topMargin: 14
        Layout.leftMargin: 16
        Layout.rightMargin: 8
        spacing: 10
        Label { id: lbl; type: "labelMedium"; color: Theme.fgSurfaceVariant }
        WavyDivider { Layout.fillWidth: true }
        IconButton { visible: parent.action !== ""; icon: parent.action; size: "xs"; iconColor: Theme.primary }
    }
}
