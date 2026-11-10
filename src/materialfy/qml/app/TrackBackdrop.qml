import QtQuick
import Md3

// Backdrop from the playing track's cover (CoverBackdrop) with double buffering: the window's
// atmospheric underlay and the fullscreen player's background. A new cover loads ahead of time into
// a hidden layer, but swaps at the same moment as the app colors (Theme.schemeApplied): under the
// shade instantly, and the shade opens the new background together with the new colors; on a smooth
// scheme change it fades in over the old one.
// If the theme does not depend on the cover (followTheme: false) — immediately once loaded; if the
// scheme has not changed — on its own, after a short delay.
Item {
    id: root

    property string source
    property string mode: "gauss"
    property real blur: 1
    property real flow: 0.5
    property bool running: true
    property bool followTheme: true

    property int _front: 0
    property bool _waiting: false          // time to swap, but the new cover is still loading
    readonly property var _layers: [a, b]
    readonly property var _back: _layers[1 - _front]

    // Translucent background (the window's atmospheric underlay): in Qt Quick, opacity applies to each
    // layer separately, so while the new cover fades in over the old one the two add up — the background
    // brightens and then, at the end of the transition, suddenly darkens. For the duration of the
    // transition, use one shared layer with a single opacity for both.
    layer.enabled: fade.running && opacity < 1

    onSourceChanged: {
        _settle()
        _back.source = source
        _waiting = false
        if (!followTheme || _layers[_front].source === "")   // nothing to wait for / first cover
            _swapOrWait(true)
        else
            fallback.restart()
    }

    function _settle() {
        fade.stop()
        _layers[_front].opacity = 1
        _back.opacity = 0
    }

    function _swapOrWait(animated) {
        const back = _back, front = _layers[_front]
        if (back.source !== source)
            return                          // nothing to swap
        if (!back.ready) {
            _waiting = true
            return
        }
        _settle()
        fallback.stop()
        _waiting = false
        back.z = 1
        front.z = 0
        _front = 1 - _front
        if (animated) {                     // the new one fades in over the old one — no dip into the theme color
            front.opacity = 1
            fade.target = back
            fade.start()
        } else {
            _settle()
        }
    }

    NumberAnimation {
        id: fade
        property: "opacity"
        from: 0
        to: 1
        duration: 700
        easing.type: Easing.InOutCubic
        onFinished: root._settle()
    }

    Connections {
        target: Theme
        function onSchemeApplied(instant) { root._swapOrWait(!instant) }
    }
    Connections {
        target: root._back
        function onReadyChanged() {
            if (root._back.ready && root._waiting)
                root._swapOrWait(true)
        }
    }
    // The scheme has not changed — swap ourselves
    Timer {
        id: fallback
        interval: 2500
        onTriggered: root._swapOrWait(true)
    }

    component Layer: CoverBackdrop {
        anchors.fill: parent
        mode: root.mode
        blur: root.blur
        flow: root.flow
        visible: opacity > 0 && ready
        running: root.running && visible
    }

    Layer { id: a }
    Layer { id: b; opacity: 0 }
}
