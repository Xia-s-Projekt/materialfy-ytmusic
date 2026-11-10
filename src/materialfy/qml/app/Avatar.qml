import QtQuick
import Md3

// Аватар: фото (source), иначе инициалы, иначе значок человека.
MorphShape {
    id: root
    property string name
    property string source
    property real size: 40

    width: size
    height: size
    implicitWidth: size
    implicitHeight: size
    shape: "circle"
    color: Theme.tertiaryContainer

    readonly property string initials: name.split(/\s+/).filter(p => p.length).slice(0, 2).map(p => p[0].toUpperCase()).join("")
    readonly property bool _photo: source !== "" && photoLoader.item !== null && photoLoader.item.status === Image.Ready

    Label {
        anchors.centerIn: parent
        visible: root.initials !== "" && !root._photo
        text: root.initials
        type: root.size >= 140 ? "displayLarge" : root.size >= 64 ? "headlineSmall" : root.size < 32 ? "labelSmall" : "titleSmall"
        weight: 600
        color: Theme.fgTertiaryContainer
    }
    Icon {
        anchors.centerIn: parent
        visible: root.initials === "" && !root._photo
        name: "person"
        size: root.size * 0.55
        fill: 1
        color: Theme.fgTertiaryContainer
    }
    Loader {
        id: photoLoader
        anchors.fill: parent
        active: root.source !== ""
        sourceComponent: MorphImage {
            source: root.source
            shape: "circle"
            placeholderColor: "transparent"
        }
    }
}
