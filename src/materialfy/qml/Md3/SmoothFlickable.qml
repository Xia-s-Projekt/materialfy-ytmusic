import QtQuick

// Flickable with smooth wheel scrolling (see SmoothScroll) for the desktop.
Flickable {
    id: root

    property alias wheelStep: scroll.wheelStep
    property alias smoothing: scroll.smoothing
    property alias showScrollBar: scroll.showScrollBar
    readonly property alias minY: scroll.minY
    readonly property alias maxY: scroll.maxY

    function scrollTo(y, animated) { scroll.scrollTo(y, animated) }

    boundsBehavior: Flickable.StopAtBounds
    acceptedButtons: Qt.NoButton
    flickableDirection: Flickable.VerticalFlick
    clip: true

    SmoothScroll {
        id: scroll
        flickable: root
        parent: root   // not in contentItem: the indicator must not scroll with the content
    }
}
