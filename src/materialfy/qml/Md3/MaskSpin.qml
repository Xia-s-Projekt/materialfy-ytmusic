import QtQuick

// Slow rotation of the frame shape on a MorphImage (target) while spinning. On stopping, the frame
// turns to the nearest quarter turn — the square must not end up crooked.
// A separate object, not part of MorphImage: there are hundreds of covers in lists, but only one or two spin.
TickAnimation {
    id: root

    property Item target
    property bool spinning: false
    property real speed: 8                 // degrees per second

    running: spinning && target !== null && target.visible && !Theme.quiet && !Theme.reducedMotion
    onTriggered: target.maskRotation = (target.maskRotation + frameTime * speed) % 360

    onSpinningChanged: {
        _settle.stop()
        if (!spinning && target) {
            _settle.to = Math.round(target.maskRotation / 90) * 90
            _settle.start()
        }
    }

    property NumberAnimation _settle: NumberAnimation {
        target: root.target
        property: "maskRotation"
        duration: Theme.motion.spatialSlow
        easing.type: Easing.BezierSpline
        easing.bezierCurve: Theme.motion.emphasizedDecelerate
    }
}
