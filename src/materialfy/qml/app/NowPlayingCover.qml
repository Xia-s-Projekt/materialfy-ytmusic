import QtQuick
import Materialfy.Core
import Md3

// Cover of the fullscreen player. A static MD3 shape: a track change is just a cross-fade,
// with no morph and no rotation. Two covers — a double buffer: the new one shows once it has loaded.
Item {
    id: root

    property string source
    property var restShape: "softSquare"
    property real pulse: 0
    property bool spinning: false
    property int direction: 1              // +1 — forward in the queue, -1 — back: which way to rotate

    property int _front: 0
    property bool _pending: false
    readonly property var _layers: [a, b]

    onSourceChanged: {
        const front = _layers[_front], back = _layers[1 - _front]
        if (front.source.toString() === "" || !visible) {   // first cover / player closed — no animation
            front.source = source
            return
        }
        if (front.source.toString() === source)
            return
        back.source = source
        _pending = true
        if (back.status === Image.Ready)
            _go()
        else
            timeout.restart()
    }

    Timer { id: timeout; interval: 1500; onTriggered: if (root._pending) root._go() }
    Connections {
        target: root._layers[1 - root._front]
        function onStatusChanged() {
            if (root._pending && root._layers[1 - root._front].status === Image.Ready)
                root._go()
        }
    }

    function _go() {
        _pending = false
        timeout.stop()
        outAnim.complete()
        inAnim.complete()
        const oldL = _layers[_front], newL = _layers[1 - _front]
        newL.jumpTo("softSquare")
        newL.opacity = 0
        newL.z = 1
        oldL.z = 0
        oldL.shape = "softSquare"
        outAnim.target = oldL
        inAnim.target = newL
        _front = 1 - _front
        newL.shape = Qt.binding(() => root.restShape)
        outAnim.start()
        inAnim.start()
    }

    component Layer: MorphImage {
        id: layer
        required property int index
        anchors.fill: parent
        eager: true                        // a closed player keeps the cover ready: opening with no loading
        fallbackKind: Player.album !== "" ? "album" : "track"
        fallbackArtist: Player.artist.split(", ")[0]
        fallbackTitle: Player.album !== "" ? Player.album : Player.title
        duration: 0
        shape: root.restShape
        pulse: index === root._front ? root.pulse : 0
        MaskSpin { target: layer; spinning: layer.index === root._front && root.spinning }
        visible: opacity > 0.01
    }

    Layer { id: a; index: 0 }
    Layer { id: b; index: 1; opacity: 0 }

    ParallelAnimation {
        id: outAnim
        property Item target
        NumberAnimation { target: outAnim.target; property: "opacity"; to: 0; duration: 180; easing.type: Easing.OutQuad }
    }
    NumberAnimation {
        id: inAnim
        property Item target
        target: inAnim.target
        property: "opacity"
        to: 1
        duration: 220
        easing.type: Easing.OutQuad
    }
}
