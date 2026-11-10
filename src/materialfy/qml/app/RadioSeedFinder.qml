import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Поповер «Радио по любому треку»: поиск по трекам Spotify (Catalog.search(text, "tracks")), 6 результатов;
// выбор → picked(uri) (сид попадает в группу «Любой трек»). Кладётся поверх страницы: anchors.fill;
// open(item) — рядом с элементом (кружок «+» на орбите). Клик мимо или Esc — закрыть.
Item {
    id: root

    property bool opened: false
    property string query
    property point _at: Qt.point(0, 0)

    signal picked(string uri, string title)

    function open(anchorItem) {
        query = ""
        const p = anchorItem ? anchorItem.mapToItem(root, 0, 0) : Qt.point(width / 2, height / 3)
        _at = p
        opened = true
        Qt.callLater(() => field.inputItem.forceActiveFocus())
    }
    function close() { opened = false }

    visible: opened || card.opacity > 0

    Timer {
        id: searchDelay
        interval: 300
        onTriggered: if (root.query.trim() !== "") Catalog.search(root.query.trim(), "tracks")
    }
    onQueryChanged: searchDelay.restart()

    MouseArea {
        anchors.fill: parent
        enabled: root.opened
        acceptedButtons: Qt.AllButtons
        onPressed: root.close()
        onWheel: w => w.accepted = true
    }

    Rectangle {
        id: card
        width: Math.min(360, root.width - 16)
        height: content.implicitHeight + 32
        x: Math.max(8, Math.min(root.width - width - 8, root._at.x - width))
        y: Math.max(8, Math.min(root.height - height - 8, root._at.y - 200))
        radius: Theme.shape.extraLarge
        color: Theme.surfaceContainerHigh
        transformOrigin: Item.BottomRight
        scale: root.opened ? 1 : 0.92
        opacity: root.opened ? 1 : 0
        Behavior on scale { enabled: Theme.reducedMotion !== true; NumberAnimation { duration: Theme.motion.spatialFast; easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedDecelerate } }
        Behavior on opacity { NumberAnimation { duration: Theme.reducedMotion === true ? 0 : Theme.motion.effectsDefault } }
        Keys.onEscapePressed: root.close()

        Elevation { target: card; level: 2 }
        MouseArea { anchors.fill: parent; acceptedButtons: Qt.AllButtons; onWheel: w => w.accepted = true }

        ColumnLayout {
            id: content
            x: 16
            y: 16
            width: parent.width - 32
            spacing: 8

            Label { Layout.leftMargin: 4; text: I18n.t("Radio from any track"); type: "titleSmall" }
            SearchField {
                id: field
                Layout.fillWidth: true
                implicitHeight: 40
                placeholder: I18n.t("Track or artist")
                text: root.query
                onTextChanged: root.query = text
                Keys.onEscapePressed: root.close()
                onAccepted: if (results.count > 0) results.itemAtIndex(0).pick()
            }

            ListView {
                id: results
                Layout.fillWidth: true
                Layout.preferredHeight: root.query.trim() === "" ? 0 : Math.min(236, Math.max(Math.min(count, 6), 1) * 56)
                clip: true
                boundsBehavior: Flickable.StopAtBounds
                model: root.query.trim() !== "" ? Catalog.searchTracks : null
                // только первые 6
                delegate: Item {
                    id: row
                    required property int index
                    required property var model
                    function pick() {
                        if (row.model.uri) {
                            root.picked(row.model.uri, row.model.title ?? "")
                            root.close()
                        }
                    }
                    width: ListView.view.width
                    height: index < 6 ? 56 : 0
                    visible: index < 6
                    RowLayout {
                        anchors.fill: parent
                        anchors.leftMargin: 4
                        anchors.rightMargin: 8
                        spacing: 12
                        MorphImage {
                            Layout.preferredWidth: 40
                            Layout.preferredHeight: 40
                            source: row.model.cover ?? ""
                            shape: "softSquare"
                        }
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 0
                            Label { Layout.fillWidth: true; text: row.model.title ?? ""; type: "bodyMedium"; weight: 500 }
                            Label { Layout.fillWidth: true; text: row.model.artists ?? ""; type: "bodySmall"; color: Theme.fgSurfaceVariant }
                        }
                        Icon { name: "radio"; size: 20; color: Theme.primary }
                    }
                    HoverHandler { id: hover }
                    StateLayer {
                        anchors.fill: parent
                        radius: Theme.shape.medium
                        onClicked: row.pick()
                    }
                }
                Label {
                    anchors.centerIn: parent
                    visible: results.count === 0 && root.query.trim() !== "" && Catalog.searchState !== "loading" && !searchDelay.running
                    text: I18n.t("Nothing found")
                    type: "bodyMedium"
                    color: Theme.fgSurfaceVariant
                }
                LoadingIndicator {
                    anchors.centerIn: parent
                    size: 32
                    visible: root.query.trim() !== "" && (Catalog.searchState === "loading" || searchDelay.running)
                }
            }
            Label {
                Layout.leftMargin: 4
                Layout.fillWidth: true
                text: I18n.t("Or use “Track radio” in any row’s menu")
                type: "bodySmall"
                color: Theme.fgSurfaceVariant
                wrapMode: Text.WordWrap
            }
        }
    }
}
