import QtQuick
import Md3

// Floating shapes behind the page header. It is placed OUTSIDE the scrollable area (otherwise its clip cuts
// the shapes off at the edge) and travels with the header on scroll; it goes under the transparent top panel, and at the top
// and bottom the shapes dissolve — there is no visible border. It runs when the header has scrolled out of sight.
Item {
    id: root

    property real headerHeight: 320        // height of the header behind which the shapes float
    property real scroll: 0                // how far the page is scrolled
    property real contentX: 0              // header area horizontally
    property real contentWidth: width
    property string seed
    property real shapesOpacity: 0.75
    property bool active: true
    property bool running: true            // false — the shapes stay put, no motion

    readonly property real above: 64       // height of the top panel above the page
    readonly property real below: 140      // space under the header for the dissolve

    visible: active && scroll < headerHeight + below
    y: -above - scroll
    height: above + headerHeight + below
    clip: true

    // Edge dissolve is computed by each shape's shader, not by a layer over them: a "live" layer with changing
    // content asks the window for an extra frame after every change (the floating shapes used to redraw the window
    // twice per tick), and on software rendering it costs a whole pass over the header by itself.
    FloatingShapes {
        x: root.contentX
        y: root.above
        width: root.contentWidth
        height: root.headerHeight
        seed: root.seed
        opacity: root.shapesOpacity
        running: root.visible && root.running
        fadeFrom: -root.above
        fadeIn: root.above * 1.6
        fadeTo: root.headerHeight + root.below
        fadeOut: root.below * 1.4
    }
}
