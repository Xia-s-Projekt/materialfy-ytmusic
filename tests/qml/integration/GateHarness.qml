// End-to-end check of the decode gate on the real MorphImage (Md3 is registered in the app):
//   QT_QPA_PLATFORM=offscreen QT_FORCE_STDERR_LOGGING=1 \
//     target/release/materialfy --offline --new-instance --qml tests/qml/integration/GateHarness.qml
// 32 covers with a real local file: the gate lets through 12 at a time, the queue must drain fully —
// no cover gets "stuck" (that is the regression fixed in MorphImage/ImageGate).
// Result — SMOKE-RESULT PASS/FAIL; the calling command checks it.
import QtQuick
import QtQuick.Controls.Basic
import Md3

ApplicationWindow {
    id: win
    width: 900
    height: 700
    visible: true

    property int readyCount: 0
    property int errorCount: 0

    Grid {
        anchors.fill: parent
        columns: 8
        Repeater {
            model: 32
            MorphImage {
                width: 96
                height: 96
                eager: true
                source: Qt.resolvedUrl("cover.png")
                onStatusChanged: {
                    if (status === Image.Ready)
                        win.readyCount += 1
                    else if (status === Image.Error)
                        win.errorCount += 1
                }
            }
        }
    }

    Timer {
        interval: 300
        repeat: true
        property int ticks: 0
        running: true
        onTriggered: {
            ticks += 1
            if (win.readyCount + win.errorCount >= 32 || ticks > 20) {
                console.log("SMOKE-PASS covers-ready", win.readyCount === 32)
                console.log("gate-slots-left", ImageGate.running, "queue", ImageGate._queue.length)
                console.log(win.readyCount === 32 && ImageGate.running === 0 && ImageGate._queue.length === 0
                            ? "SMOKE-RESULT PASS" : "SMOKE-RESULT FAIL")
                Qt.quit()
            }
        }
    }
}
