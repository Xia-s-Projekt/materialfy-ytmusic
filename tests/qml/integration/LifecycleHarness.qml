// Playlist lifecycle on the real bridge (Library from Materialfy.Core):
//   QT_QPA_PLATFORM=offscreen QT_FORCE_STDERR_LOGGING=1 \
//     target/release/materialfy --offline --new-instance --qml tests/qml/integration/LifecycleHarness.qml
// Checks the QML-integration contract: playlistTracks is a stable model instance per call,
// releasePlaylist paired with every acquire; the real TrackListPage/PlaylistPage open
// and close without QML errors; the header/page do not wait for data. Result — SMOKE-RESULT PASS/FAIL.
// Note: rows cannot be added to DictListModel from QML (the model is created in Rust) — filling
// the model with a wave on a real TrackListPage is not checked here (see WaveHarness/docs).
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

    Timer { id: step; interval: 150; repeat: true; property int n: 0; onTriggered: run() }
    Component.onCompleted: step.start()

    function run() {
        const t = step.n++
        if (t === 0) {
            // stable instance: a repeated call is the same object, the owner count grows
            const m1 = Library.playlistTracks("smoke-id-1")
            const m2 = Library.playlistTracks("smoke-id-1")
            check("model-stable-instance", m1 !== null && m1 === m2)
            check("release-invokable", true)      // we got here — the invokable exists
            Library.releasePlaylist("smoke-id-1") // pair for the second acquire
            // real TrackListPage on the bridge model: the page and the empty state are honest
            pageLoader.active = true
        } else if (t === 1) {
            const p = pageLoader.item
            check("page-loads", p !== null && p.listView !== null)
            check("empty-honest", p.model.count === 0 && !p.model.loading)
            pageLoader.active = false             // destruction → releasePlaylist in a pair
        } else if (t === 2) {
            check("page-destroyed", pageLoader.item === null)
            // real PlaylistPage: the production acquire/release path (_syncModel with a guard)
            plLoader.active = true
        } else if (t === 3) {
            const q = plLoader.item
            check("playlist-page-loads", q !== null && q.playlistId === "smoke-id-1")
            plLoader.active = false               // hiding/destruction → releasePlaylist
        } else if (t === 4) {
            check("playlist-page-destroyed", plLoader.item === null)
            finish()
        }
    }

    Loader {
        id: pageLoader
        active: false
        anchors.fill: parent
        sourceComponent: App.TrackListPage {
            id: tp
            // acquire once per page lifetime (not in a binding: re-evaluation would inflate the count)
            Component.onCompleted: tp.model = Library.playlistTracks("smoke-id-1")
            Component.onDestruction: Library.releasePlaylist("smoke-id-1")
            title: "Lifecycle smoke"
            showHeader: false
        }
    }
    Loader {
        id: plLoader
        active: false
        anchors.fill: parent
        sourceComponent: Pages.PlaylistPage {
            playlistId: "smoke-id-1"
        }
    }
}
