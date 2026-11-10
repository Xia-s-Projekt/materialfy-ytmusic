// Router navigation scenarios on the real bridge (Md3 singletons from the app):
//   QT_QPA_PLATFORM=offscreen QT_FORCE_STDERR_LOGGING=1 \
//     target/release/materialfy --offline --new-instance --qml tests/qml/integration/RouterHarness.qml
// 1. Loading LibraryPage (a duplicate of the prewarmEntry method used to bring down the whole file).
// 2. reset("settings") from a playlist: settings is a keepAlive page whose live instance sits deep
//    in the stack; reset must replace the stack (depth 1, current settings), not get lost.
// 3. A click during hover-preloading of the same page: the preload is upgraded to a forward transition.
// Result — SMOKE-RESULT PASS/FAIL.
import QtQuick
import QtQuick.Controls.Basic
import Md3
import Materialfy.Core
import "../../../src/materialfy/qml/app" as App
import "../../../src/materialfy/qml/app/pages" as Pages

ApplicationWindow {
    id: win
    width: 900
    height: 700
    visible: true

    property int _fails: 0
    function check(name, cond) {
        console.log(cond ? "SMOKE-PASS " + name : "SMOKE-FAIL " + name)
        if (!cond) _fails++
    }
    function finish() {
        console.log(_fails === 0 ? "SMOKE-RESULT PASS" : "SMOKE-RESULT FAIL")
        Qt.quit()
    }

    App.Router {
        id: router
        anchors.fill: parent
        keepAliveSlots: 6
        // settings must be a keepAlive page: otherwise scenario 2 builds a fresh page
        // and passes even with a broken reset (the in-stack _kept path is not checked)
        keepAlive: ["home", "settings"]
        pages: ({
            home: Qt.resolvedUrl(Qt.resolvedUrl("../../../src/materialfy/qml/app/pages/HomePage.qml")),
            playlist: Qt.resolvedUrl(Qt.resolvedUrl("../../../src/materialfy/qml/app/pages/PlaylistPage.qml")),
            settings: Qt.resolvedUrl(Qt.resolvedUrl("../../../src/materialfy/qml/app/pages/SettingsPage.qml")),
            library: Qt.resolvedUrl(Qt.resolvedUrl("../../../src/materialfy/qml/app/pages/LibraryPage.qml")),
        })
        Component.onCompleted: reset("home")
    }

    // Scenario 1: LibraryPage loads (the Library bridge is real)
    Timer { interval: 400; running: true; onTriggered: {
        let loaded = false
        try {
            const comp = Qt.createComponent(Qt.resolvedUrl("../../../src/materialfy/qml/app/pages/LibraryPage.qml"))
            loaded = comp.status === Component.Ready
            if (comp.status === Component.Error)
                console.log("LibraryPage error:", comp.errorString())
        } catch (e) {
            console.log("LibraryPage threw:", e)
        }
        check("library page compiles", loaded)

        // Scenario 2: reset to a keepAlive page whose instance is alive deep in the stack.
        // First open settings (it goes into _kept), go back to home, open
        // a playlist, then reset("settings").
        router.reset("home")
        Qt.callLater(runScenario2)
    } }

    function runScenario2() {
        router.push("settings")
        timerToScenario2b.restart()
    }
    Timer { id: timerToScenario2b; interval: 1200; onTriggered: runScenario2b() }
    function runScenario2b() {
        // settings was incubated asynchronously and lives in _kept; go to home and into a playlist
        router.reset("home")
        router.push("playlist", { playlistId: "r", playlistTitle: "R", cover: "" })
        timerToScenario2c.restart()
    }
    Timer { id: timerToScenario2c; interval: 900; onTriggered: runScenario2c() }
    function runScenario2c() {
        router.reset("settings")
        // settings incubation is asynchronous — check after it completes
        timerToCheck2.restart()

    }
    Timer { id: timerToCheck2; interval: 800; onTriggered: {
        check("reset to in-stack keepAlive page replaces the stack",
              router.depth === 1 && router.currentName === "settings")
        startScenario3()
    } }
    function startScenario3() {
        router.reset("home")
        router.prewarmKeyed("playlist", { playlistId: "w", playlistTitle: "W", cover: "" })
        check("prewarm pending", router._pending !== null && router._pending.kind === "prewarm")
        router.push("playlist", { playlistId: "w", playlistTitle: "W", cover: "" })
        timerToScenario3.restart()
    }
    Timer { id: timerToScenario3; interval: 900; onTriggered: {
        check("click during prewarm lands on the page",
              router.currentName === "playlist" && router.depth >= 1)
        finish()
    } }
}
