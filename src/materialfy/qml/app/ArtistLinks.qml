import QtQuick
import Md3
import Materialfy.Core

// Comma-separated artists: each one is its own link. Hovering a name shows that artist's card
// and clicking opens their page — with no picker menu when there are several artists.
// If the names do not fit on one line, the plain label with "…" stays: cutting a name in half is worse.
Item {
    id: root

    // [{ id, name }]; empty — just the text label (episode, old rows without links)
    property var refs: []
    property string text: ""
    property string type: "bodyMedium"
    property color color: Theme.fgSurfaceVariant
    property bool centered: false
    // Artist card on hover. The bottom panel does not need it: there the name is just a path to their
    // page, while the card would pop up over the player and cover the row the click targets
    property bool preview: true

    readonly property var _refs: refs ?? []
    readonly property bool fits: width <= 0 || _names.advanceWidth <= width
    // the names from refs must match the label: otherwise the row is shown as-is, without links
    readonly property bool _matches: text === "" || _names.text === text
    readonly property bool linked: _refs.length > 0 && fits && _matches
    // content width — without looking at width: otherwise the layout gets a binding loop
    readonly property real naturalWidth: _refs.length > 0 && _matches ? _names.advanceWidth : _label.advanceWidth

    implicitWidth: naturalWidth
    implicitHeight: Math.ceil(_probe.implicitHeight)
    clip: true

    Label {
        id: _probe
        visible: false
        text: "X"
        type: root.type
    }
    TextMetrics {
        id: _names
        font: _probe.font
        text: root._refs.map(r => r.name ?? "").join(", ")
    }
    TextMetrics {
        id: _label
        font: _probe.font
        text: root.text
    }

    Label {
        anchors.fill: parent
        visible: !root.linked
        text: root.text
        type: root.type
        color: root.color
        elide: Text.ElideRight
        horizontalAlignment: root.centered ? Text.AlignHCenter : Text.AlignLeft
    }

    Row {
        visible: root.linked
        anchors.top: parent.top
        anchors.left: root.centered ? undefined : parent.left
        anchors.horizontalCenter: root.centered ? parent.horizontalCenter : undefined
        spacing: 0

        Repeater {
            model: root._refs

            Row {
                required property var modelData
                required property int index

                Label {
                    visible: index > 0
                    text: ", "
                    type: root.type
                    color: root.color
                }
                LinkLabel {
                    text: modelData.name ?? ""
                    type: root.type
                    color: root.color
                    linkEnabled: (modelData.id ?? "") !== ""
                    previewArtistId: root.preview ? (modelData.id ?? "") : ""
                    onClicked: Catalog.openArtist(modelData.id ?? "")
                }
            }
        }
    }
}
