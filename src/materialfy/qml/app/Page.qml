import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core

// Base page: smooth scrolling, large title, bottom padding for the floating mini player.
// Content goes directly inside: Page { title: "…"; Label { … } } — it lands in a ColumnLayout.
Item {
    id: page

    property var router
    property string title
    property string subtitle
    property bool showHeader: true
    property real sideMargin: 32
    property real bottomInset: 120
    default property alias content: column.data
    // A layer under the content, outside the scrollable area — its clip does not cut it (header background, see HeroShapes)
    property alias underlay: underlayHost.data
    readonly property alias flickable: flick
    readonly property bool scrolled: flick.contentY > 48

    Item { id: underlayHost; anchors.fill: parent }

    SmoothFlickable {
        id: flick
        anchors.fill: parent
        contentHeight: column.implicitHeight + 8 + page.bottomInset
        wheelStep: Settings.wheelStep

        ColumnLayout {
            id: column
            x: page.sideMargin
            y: 8
            width: flick.width - 2 * page.sideMargin
            spacing: 24

            ColumnLayout {
                visible: page.showHeader
                Layout.fillWidth: true
                spacing: 4
                Label {
                    Layout.fillWidth: true
                    text: page.title
                    type: "displaySmall"
                    weight: 600
                }
                Label {
                    visible: page.subtitle !== ""
                    Layout.fillWidth: true
                    text: page.subtitle
                    type: "bodyLarge"
                    color: Theme.fgSurfaceVariant
                }
            }
        }
    }
}
