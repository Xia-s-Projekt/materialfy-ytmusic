import QtQuick
import QtQuick.Layouts
import Md3

// Две карточки входа: 1 — вход в YouTube Music (медиатека, поиск), 2 — воспроизведение здесь.
// Один вход через заголовки открывает и то, и другое.
// Активная — обводка primary 2 px, у выполненного номер морфится в «солнце» с галочкой;
// шаг 2 до выполнения шага 1 — surfaceContainerLow. В узкой колонке карточки идут друг под другом.
GridLayout {
    id: root

    property int active: 1                 // 0 (ни один) | 1 | 2
    property bool done1: false
    property bool done2: false
    property bool compact: false           // сжатые тексты (рейл, узкое окно)
    property string waitingText: ""        // подпись вместо метки шага 1 («Ждём подтверждения…»)

    readonly property bool _instant: Theme.reducedMotion === true

    columns: width >= 520 ? 2 : 1
    columnSpacing: 12
    rowSpacing: 12

    component StepCard: Rectangle {
        id: card
        property int number: 1
        property string title
        property string text
        property string tag
        property string tagIcon
        property bool isActive: false
        property bool done: false
        property bool later: false
        property bool instant: false

        Layout.fillWidth: true
        Layout.fillHeight: true
        Layout.preferredWidth: 0
        implicitHeight: col.implicitHeight + 36
        radius: Theme.shape.extraLarge
        // «позже» — утоплен: в тёмной теме Low темнее фона страницы, в светлой Low светлее (выглядел бы
        // приподнятым), поэтому там — цвет фона и тонкая обводка
        color: !later ? Theme.surfaceContainerHigh : Theme.dark ? Theme.surfaceContainerLow : Theme.surfaceContainer
        border.width: isActive ? 2 : later && !Theme.dark ? 1 : 0
        border.color: isActive ? Theme.primary : Theme.outlineVariant
        Behavior on color { enabled: !Theme.transitioning; ColorAnimation { duration: Theme.motion.effectsDefault } }
        Behavior on border.width { NumberAnimation { duration: Theme.motion.effectsDefault } }

        ColumnLayout {
            id: col
            x: 20
            y: 18
            width: parent.width - 40
            spacing: 8

            RowLayout {
                Layout.fillWidth: true
                spacing: 12
                MorphShape {
                    Layout.preferredWidth: 40
                    Layout.preferredHeight: 40
                    shape: card.done ? "sunny" : "cookie9"
                    duration: card.instant ? 0 : Theme.motion.spatialDefault
                    overshoot: 1.15
                    color: card.isActive || card.done ? Theme.primary : Theme.secondaryContainer
                    Label {
                        anchors.centerIn: parent
                        visible: !card.done
                        text: String(card.number)
                        type: "titleMedium"
                        weight: 600
                        color: card.isActive ? Theme.fgPrimary : Theme.fgSecondaryContainer
                    }
                    Icon {
                        anchors.centerIn: parent
                        visible: card.done
                        name: "check"
                        size: 22
                        weight: 600
                        color: Theme.fgPrimary
                    }
                }
                Label {
                    Layout.fillWidth: true
                    text: card.title
                    type: root.compact ? "titleSmall" : "titleMedium"
                    wrapMode: Text.WordWrap
                    elide: Text.ElideNone
                }
            }
            Label {
                Layout.fillWidth: true
                visible: card.text !== ""
                text: card.text
                type: "bodySmall"
                color: Theme.fgSurfaceVariant
                wrapMode: Text.WordWrap
                elide: Text.ElideNone
                lineHeightMode: Text.ProportionalHeight
                lineHeight: 1.2
            }
            Rectangle {
                visible: card.tag !== ""
                Layout.preferredWidth: tagRow.implicitWidth + 20
                Layout.preferredHeight: 24
                radius: Theme.shape.small
                color: Theme.surfaceContainerHighest
                Row {
                    id: tagRow
                    anchors.centerIn: parent
                    spacing: 4
                    Icon {
                        visible: card.tagIcon !== ""
                        anchors.verticalCenter: parent.verticalCenter
                        name: card.tagIcon
                        size: 14
                        color: Theme.fgSurfaceVariant
                    }
                    Label {
                        anchors.verticalCenter: parent.verticalCenter
                        text: card.tag
                        type: "labelSmall"
                        color: Theme.fgSurfaceVariant
                    }
                }
            }
        }
    }

    StepCard {
        number: 1
        title: I18n.t("Library")
        // в сжатом виде — одна строка вместо описания и метки
        text: root.compact ? (root.done1 ? I18n.t("Done") : root.waitingText !== "" ? root.waitingText : I18n.t("Playlists, search"))
                           : I18n.t("Playlists, Liked Songs, search and history.")
        tag: root.compact ? "" : root.done1 ? I18n.t("Done") : root.waitingText !== "" ? root.waitingText : I18n.t("One sign-in · now")
        tagIcon: root.done1 ? "check" : root.waitingText !== "" ? "schedule" : "login"
        isActive: root.active === 1 && !root.done1
        done: root.done1
        instant: root._instant
    }
    StepCard {
        number: 2
        title: root.compact ? I18n.t("Listen here") : I18n.t("Listen on this computer")
        text: root.compact ? (root.done2 ? I18n.t("Done") : I18n.t("Same sign-in"))
                           : I18n.t("The same sign-in turns on playback here: pick a track and it plays on this computer.")
        tag: root.compact ? "" : root.done2 ? I18n.t("Done") : I18n.t("Same sign-in")
        tagIcon: root.done2 ? "check" : "play_arrow"
        isActive: root.active === 2 && !root.done2
        done: root.done2
        later: !root.done1
        instant: root._instant
    }
}
