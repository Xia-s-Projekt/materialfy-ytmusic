import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core
import ".."

// Аккаунт. До входа — «Войти в YouTube Music»: вставляем заголовки браузера (headers-auth JSON)
// в поле ниже — приложение само проверяет и сохраняет. После входа — сводка аккаунта.
// Смена состояния — сквозное затухание.
Page {
    id: page

    property string st: Auth.state
    readonly property var account: Auth.account ?? {}
    readonly property bool premium: Auth.premium === true
    readonly property string displayName: account.displayName ?? ""
    readonly property bool _instant: Theme.reducedMotion === true
    // вход через сессию отказал: аккаунт Free — предложить вход без Premium
    readonly property bool freeRefused: Auth.errorText.indexOf("Free") >= 0
    readonly property bool playbackHere: Auth.playbackState === "ready"

    // «Готово, {имя}» — только если вход прошёл через браузер, пока страница открыта (а не восстановилась сессия)
    property bool _sawBrowser: st === "awaitingBrowser"
    onStChanged: {
        if (st === "awaitingBrowser")
            _sawBrowser = true
        else if (st === "signedOut")
            _sawBrowser = false
    }
    readonly property string view: st === "signedIn" ? (_sawBrowser ? "welcome" : "account")
                                 : st === "checking" ? "checking"
                                 : st === "awaitingBrowser" ? "browser"
                                 : st === "connecting" ? "connecting"
                                 : st === "error" ? "error" : "signedOut"
    property string shown: view
    onViewChanged: {
        if (_instant || !visible) {
            swap.stop()
            shown = view
            body.opacity = 1
            body.scale = 1
            shift.y = 0
        } else {
            swap.restart()
        }
    }
    readonly property bool login: shown !== "account"
    // рейл / низкое окно: меньше арт, короткие тексты, шаги без меток
    readonly property bool compact: width < 860 || height < 700

    title: login ? I18n.t("Sign-in") : I18n.t("Account")
    showHeader: false

    function _go(name, props) {
        if (page.router)
            page.router.push(name, props)
    }
    function _proxy() { _go("settings", { section: "network" }) }

    // Пояснения к ошибкам входа (тексты ошибок — из Auth.errorText, уже переведены в Rust;
    // подсказку выбираем по стабильному коду Auth.errorCode)
    function _errorHint(text) {
        const code = text !== "" ? (Auth.errorCode ?? "") : ""
        if (code === "expired" || text.indexOf("expired") >= 0)
            return I18n.t("Your copied headers have expired. Open music.youtube.com, copy the headers again and paste them here.")
        if (code === "cancelled")
            return I18n.t("The sign-in was cancelled. Copy the headers from music.youtube.com again and paste them here.")
        if (code === "noResponse")
            return I18n.t("Check your internet connection. If YouTube Music is only reachable through a proxy, turn it on in network settings.")
        if (code === "badJson" || text.indexOf("JSON") >= 0)
            return I18n.t("The pasted text is not valid headers JSON. Copy the browser request headers again from music.youtube.com.")
        return ""
    }

    SequentialAnimation {
        id: swap
        NumberAnimation { target: body; property: "opacity"; to: 0; duration: Theme.motion.effectsFast; easing.type: Easing.InCubic }
        ScriptAction { script: page.shown = page.view }
        ParallelAnimation {
            NumberAnimation { target: body; property: "opacity"; to: 1; duration: Theme.motion.effectsDefault; easing.type: Easing.OutCubic }
            NumberAnimation {
                target: shift; property: "y"; from: 8; to: 0; duration: Theme.motion.spatialFast
                easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedDecelerate
            }
            NumberAnimation {
                target: body; property: "scale"; from: 0.98; to: 1; duration: Theme.motion.spatialFast
                easing.type: Easing.BezierSpline; easing.bezierCurve: Theme.motion.emphasizedDecelerate
            }
        }
    }

    Item {
        Layout.fillWidth: true
        implicitHeight: body.implicitHeight

        ColumnLayout {
            id: body
            width: parent.width
            transform: Translate { id: shift }
            spacing: 0

            // ================= Вход =================
            ColumnLayout {
                visible: page.login
                Layout.fillWidth: true
                Layout.maximumWidth: page.compact ? 560 : 640
                Layout.alignment: Qt.AlignHCenter
                Layout.topMargin: page.compact ? 0 : 8
                spacing: page.compact ? 10 : 12

                // Арт: фигуры (не вошли / ошибка / готово) или индикатор (ждём, подключаемся)
                Item {
                    id: art
                    Layout.alignment: Qt.AlignHCenter
                    Layout.preferredWidth: page.compact ? 120 : 156
                    Layout.preferredHeight: page.compact ? 120 : 156
                    Layout.bottomMargin: page.compact ? 0 : 4
                    readonly property bool waiting: page.shown === "checking" || page.shown === "browser" || page.shown === "connecting"
                    readonly property bool failed: page.shown === "error"
                    readonly property bool done: page.shown === "welcome"

                    MorphShape {
                        visible: !art.waiting
                        anchors.centerIn: parent
                        width: art.width
                        height: art.height
                        shape: art.failed ? "flower8" : art.done ? "sunny" : "cookie12"
                        duration: page._instant ? 0 : Theme.motion.spatialSlow
                        overshoot: 1.05
                        color: art.failed ? Theme.errorContainer : Theme.primaryContainer
                        opacity: 0.35
                        rotation: art.done ? 20 : 12
                        Behavior on rotation { enabled: !page._instant; NumberAnimation { duration: Theme.motion.spatialSlow; easing.type: Easing.OutCubic } }
                    }
                    MorphShape {
                        visible: !art.waiting
                        anchors.centerIn: parent
                        width: Math.round(art.width * 120 / 156)
                        height: width
                        shape: art.failed ? "flower6" : art.done ? "sunny" : "cookie9"
                        duration: page._instant ? 0 : Theme.motion.spatialDefault
                        overshoot: 1.15
                        color: art.failed ? Theme.errorContainer : art.done ? Theme.primary : Theme.secondaryContainer
                        Icon {
                            anchors.centerIn: parent
                            name: art.failed ? "cloud_off" : art.done ? "check" : "library_music"
                            size: page.compact ? 36 : 46
                            fill: 1
                            color: art.failed ? Theme.fgErrorContainer : art.done ? Theme.fgPrimary : Theme.fgSecondaryContainer
                        }
                    }
                    LoadingIndicator {
                        visible: art.waiting
                        anchors.centerIn: parent
                        size: page.compact ? 80 : 96
                        contained: true
                        running: !Theme.calm && !page._instant
                    }
                }

                Label {
                    Layout.fillWidth: true
                    horizontalAlignment: Text.AlignHCenter
                    text: ({
                        checking: I18n.t("Connecting to YouTube Music…"),
                        connecting: I18n.t("Connecting to YouTube Music…"),
                        browser: I18n.t("Confirm sign-in in your browser"),
                        error: I18n.t("Couldn't sign in"),
                        welcome: page.displayName !== "" ? I18n.t("All set, {0}", page.displayName) : I18n.t("All set")
                    })[page.shown] ?? I18n.t("Sign in to YouTube Music")
                    type: page.compact ? "headlineSmall" : "headlineMedium"
                    wrapMode: Text.WordWrap
                    elide: Text.ElideNone
                }
                Label {
                    Layout.fillWidth: true
                    Layout.maximumWidth: 460
                    Layout.alignment: Qt.AlignHCenter
                    visible: text !== ""
                    horizontalAlignment: Text.AlignHCenter
                    text: ({
                        checking: I18n.t("Checking your saved sign-in"),
                        connecting: I18n.t("Checking your account and connecting to YouTube Music"),
                        browser: I18n.t("Paste the headers JSON below — the app validates and saves it itself."),
                        error: "",
                        welcome: page.playbackHere
                                 ? I18n.t("Library, search and music on this computer are ready.")
                                 : I18n.t("Library and search are ready. Music on this computer starts when you play a track.")
                    })[page.shown] ?? (page.compact ? I18n.t("Paste the headers from music.youtube.com below — the app validates and saves them itself.") : I18n.t("YouTube Music has no desktop OAuth: copy the request headers from music.youtube.com (DevTools → Network) and paste them below. The app validates and saves them itself — no files, no terminal.") + " "
                                       + (Qt.platform.os === "windows" ? I18n.t("Keys are stored in Windows Credential Manager.")
                                          : Qt.platform.os === "osx" ? I18n.t("Keys are stored in the macOS Keychain.")
                                          : I18n.t("Keys are stored in the system keychain.")))
                    type: "bodyMedium"
                    color: Theme.fgSurfaceVariant
                    wrapMode: Text.WordWrap
                    elide: Text.ElideNone
                    lineHeightMode: Text.ProportionalHeight
                    lineHeight: 1.25
                }

                // Плашка ошибки
                Rectangle {
                    visible: page.shown === "error"
                    Layout.fillWidth: true
                    Layout.topMargin: 4
                    implicitHeight: errRow.implicitHeight + 24
                    radius: Theme.shape.large
                    color: Theme.errorContainer
                    RowLayout {
                        id: errRow
                        x: 16
                        y: 12
                        width: parent.width - 32
                        spacing: 12
                        Icon { Layout.alignment: Qt.AlignTop; name: "error"; size: 20; color: Theme.fgErrorContainer }
                        ColumnLayout {
                            Layout.fillWidth: true
                            spacing: 2
                            Label {
                                Layout.fillWidth: true
                                text: Auth.errorText !== "" ? Auth.errorText : I18n.t("YouTube Music didn't respond")
                                type: "titleSmall"
                                color: Theme.fgErrorContainer
                                wrapMode: Text.WordWrap
                                elide: Text.ElideNone
                            }
                            Label {
                                Layout.fillWidth: true
                                visible: text !== ""
                                text: page._errorHint(Auth.errorText)
                                type: "bodySmall"
                                color: Theme.fgErrorContainer
                                wrapMode: Text.WordWrap
                                elide: Text.ElideNone
                            }
                        }
                    }
                }

                LoginSteps {
                    visible: page.shown !== "checking"
                    Layout.fillWidth: true
                    Layout.topMargin: page.compact ? 6 : 12
                    compact: page.compact
                    // выделен шаг, который выполняется сейчас; до нажатия «Войти» — ни один
                    active: page.shown === "welcome" ? 2 : page.shown === "browser" || page.shown === "connecting" ? 1 : 0
                    done1: page.shown === "welcome"
                    done2: page.shown === "welcome" && page.playbackHere
                    waitingText: page.shown === "browser" ? I18n.t("Waiting for confirmation…") : page.shown === "connecting" ? I18n.t("Signing in…") : ""
                }

                // Действия
                RowLayout {
                    visible: page.shown !== "checking" && page.shown !== "connecting"
                    Layout.alignment: Qt.AlignHCenter
                    Layout.topMargin: 8
                    spacing: 12

                    Button {
                        visible: page.shown === "signedOut"
                        text: I18n.t("Sign in with YouTube Music")
                        icon: "open_in_new"
                        size: "m"
                        onClicked: Auth.startLogin()
                    }
                    Button {
                        visible: page.shown === "error"
                        text: I18n.t("Retry")
                        icon: "refresh"
                        size: "m"
                        onClicked: Auth.retry()
                    }
                    Button {
                        visible: page.shown === "signedOut" || page.shown === "error"
                        text: I18n.t("Proxy settings")
                        icon: "lan"
                        style: "text"
                        onClicked: page._proxy()
                    }
                    Button {
                        visible: page.shown === "welcome"
                        text: I18n.t("Get started")
                        icon: "arrow_forward"
                        size: "m"
                        onClicked: page.router ? page.router.reset("home") : null
                    }
                }

                // Вход без файла: копируем JSON из ytmusicapi (шапка браузера музыка.youtube.com)
                // и вставляем сюда; приложение проверяет и сохраняет само.
                ColumnLayout {
                    visible: page.shown === "signedOut" || page.shown === "error"
                    Layout.fillWidth: true
                    Layout.topMargin: 12
                    Layout.maximumWidth: page.compact ? 560 : 640
                    Layout.alignment: Qt.AlignHCenter
                    spacing: 8

                    Label {
                        Layout.fillWidth: true
                        text: I18n.t("Or paste your headers-auth JSON here — no files, no terminal.")
                        type: "titleSmall"
                        wrapMode: Text.WordWrap
                        elide: Text.ElideNone
                    }
                    Rectangle {
                        id: headersBox
                        Layout.fillWidth: true
                        Layout.preferredHeight: 120
                        radius: Theme.shape.large
                        color: Theme.surfaceContainerHighest
                        border.width: headersEdit.activeFocus ? 2 : 0
                        border.color: Theme.primary
                        TextEdit {
                            id: headersEdit
                            anchors.fill: parent
                            anchors.margins: 12
                            wrapMode: TextEdit.Wrap
                            color: Theme.fgSurface
                            selectionColor: Theme.primaryContainer
                            selectedTextColor: Theme.fgPrimaryContainer
                            font.family: Theme.fontFamilyMonospace ?? Theme.fontFamily
                            font.pixelSize: Theme.type.bodyMedium.size
                            placeholderText: "{\"authorization\": \"SAPISIDHASH ...\", \"cookie\": \"...\"}"
                        }
                    }
                    Label {
                        id: headersErrorLabel
                        visible: text !== ""
                        Layout.fillWidth: true
                        text: ""
                        type: "bodySmall"
                        color: Theme.error
                        wrapMode: Text.WordWrap
                        elide: Text.ElideNone
                    }
                    RowLayout {
                        Layout.fillWidth: true
                        spacing: 12
                        Button {
                            text: I18n.t("Save & sign in")
                            icon: "login"
                            size: "m"
                            enabled: headersEdit.text.trim() !== ""
                            onClicked: {
                                const err = Auth.saveHeaders(headersEdit.text)
                                headersErrorLabel.text = err
                            }
                        }
                        Button {
                            text: I18n.t("How to get the JSON")
                            icon: "help"
                            style: "text"
                            onClicked: Qt.openUrlExternally("https://github.com/sigma67/ytmusicapi#setup")
                        }
                    }
                }

                Label {
                    visible: !page.compact && (page.shown === "signedOut" || page.shown === "error")
                    Layout.alignment: Qt.AlignHCenter
                    Layout.topMargin: 24
                    text: I18n.t("Materialfy YTMusic fork · embedded ytmusicapi, nothing to install")
                    type: "bodySmall"
                    color: Theme.fgSurfaceVariant
                }
            }

            // ================= Аккаунт =================
            // Шапка профиля во всю ширину, под ней — медиатека счётчиками и состояние этого компьютера
            ColumnLayout {
                id: acc
                visible: !page.login
                Layout.fillWidth: true
                spacing: 32
                readonly property bool narrow: width < 720
                readonly property string country: {
                    const code = page.account.country ?? ""
                    const name = code !== "" ? Qt.locale("en_" + code).nativeTerritoryName : ""
                    return name !== "" ? name : code
                }

                GridLayout {
                    Layout.fillWidth: true
                    Layout.topMargin: 16
                    columns: acc.narrow ? 1 : 2
                    columnSpacing: 32
                    rowSpacing: 20

                    Item {
                        readonly property int side: acc.narrow ? 120 : 184
                        Layout.preferredWidth: side
                        Layout.preferredHeight: side
                        Layout.alignment: Qt.AlignVCenter
                        Avatar { anchors.fill: parent; size: parent.side; name: page.displayName }
                        MorphImage {
                            anchors.fill: parent
                            visible: (page.account.avatar ?? "") !== "" && status === Image.Ready
                            source: page.account.avatar ?? ""
                            shape: "cookie9"
                        }
                    }

                    ColumnLayout {
                        Layout.fillWidth: true
                        Layout.alignment: Qt.AlignVCenter
                        spacing: 6
                        Label {
                            text: I18n.t("YouTube Music account").toUpperCase()
                            type: "labelLarge"
                            weight: 600
                            font.letterSpacing: 1.2
                            color: Theme.primary
                        }
                        Label {
                            Layout.fillWidth: true
                            text: page.displayName !== "" ? page.displayName : I18n.t("YouTube Music account")
                            type: acc.narrow ? "headlineLarge" : "displayMedium"
                            weight: 700
                        }
                        Label {
                            Layout.fillWidth: true
                            text: [acc.country, page.account.email ?? ""].filter(x => x).join(" · ")
                            type: "bodyLarge"
                            color: Theme.fgSurfaceVariant
                        }
                        Flow {
                            Layout.fillWidth: true
                            Layout.topMargin: 14
                            spacing: 8
                            Button { text: I18n.t("Playback"); icon: "speaker"; style: "tonal"; onClicked: page._go("playback") }
                            Button {
                                text: I18n.t("Open in YouTube Music")
                                icon: "open_in_new"
                                style: "outlined"
                                onClicked: Qt.openUrlExternally("https://music.youtube.com")
                            }
                            Button { text: I18n.t("Sign out"); icon: "logout"; style: "outlined"; onClicked: Auth.signOut() }
                        }
                    }
                }

                // Медиатека: счётчики, клик открывает раздел
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 12
                    SectionHeader { text: I18n.t("Your library") }
                    GridLayout {
                        id: stats
                        Layout.fillWidth: true
                        columns: Math.max(2, Math.min(4, Math.floor((width + columnSpacing) / 220)))
                        columnSpacing: 12
                        rowSpacing: 12
                        StatCard { icon: "favorite"; value: Library.liked ? Library.liked.count : 0; label: I18n.t("Liked Songs"); onClicked: page._go("liked") }
                        StatCard { icon: "queue_music"; value: Library.playlists ? Library.playlists.count : 0; label: I18n.t("Playlists"); onClicked: page._go("library", { kind: "playlists" }) }
                        StatCard { icon: "album"; value: Library.albums ? Library.albums.count : 0; label: I18n.t("Albums"); onClicked: page._go("library", { kind: "albums" }) }
                        StatCard { icon: "person"; value: Library.artists ? Library.artists.count : 0; label: I18n.t("Artists"); onClicked: page._go("library", { kind: "artists" }) }
                    }
                }

                // Этот компьютер: звук и где лежат ключи входа
                ColumnLayout {
                    Layout.fillWidth: true
                    spacing: 12
                    SectionHeader { text: I18n.t("This computer") }
                    GridLayout {
                        Layout.fillWidth: true
                        columns: acc.narrow ? 1 : 2
                        columnSpacing: 12
                        rowSpacing: 12
                        InfoCard {
                            icon: Auth.playbackState === "ready" ? "computer" : "cast"
                            title: Auth.playbackState === "ready" ? I18n.t("Playing on this computer")
                                 : I18n.t("Playback here isn't set up")
                            text: Auth.playbackState === "ready" ? I18n.t("Music plays on this computer.")
                                : I18n.t("Pick a track and it plays here — no other devices needed.")
                            action: I18n.t("Playback")
                            onTriggered: page._go("playback")
                        }
                        InfoCard {
                            icon: "lock"
                            title: Auth.tokenStorage === "keyring"
                                   ? (Qt.platform.os === "windows" ? I18n.t("Keys in Credential Manager") : I18n.t("Keys in the keychain"))
                                   : I18n.t("Sign-in")
                            text: I18n.t("Signing out removes this computer's saved headers from storage. To fully sign out everywhere, remove them in your Google account.")
                            action: I18n.t("Open music.youtube.com")
                            onTriggered: Qt.openUrlExternally("https://music.youtube.com")
                        }
                    }
                }
            }
        }
    }

    // Счётчик медиатеки: число крупно, подпись, значок; вся карточка — кнопка
    component StatCard: Rectangle {
        id: stat
        property string icon
        property int value
        property string label
        signal clicked()
        Layout.fillWidth: true
        implicitHeight: 112
        radius: Theme.shape.large
        color: Theme.surfaceContainerHigh
        Icon {
            x: 20
            y: 20
            name: stat.icon
            size: 22
            color: Theme.primary
        }
        Column {
            x: 20
            anchors.bottom: parent.bottom
            anchors.bottomMargin: 16
            width: parent.width - 40
            Label { width: parent.width; text: stat.value.toLocaleString(Qt.locale(), "f", 0); type: "headlineMedium"; weight: 700 }
            Label { width: parent.width; text: stat.label; type: "bodyMedium"; color: Theme.fgSurfaceVariant }
        }
        StateLayer { anchors.fill: parent; radius: stat.radius; onClicked: stat.clicked() }
    }

    // Карточка состояния: значок, заголовок, пояснение, кнопка
    component InfoCard: Rectangle {
        id: info
        property string icon
        property string title
        property string text
        property string action
        signal triggered()
        Layout.fillWidth: true
        Layout.fillHeight: true
        implicitHeight: infoCol.implicitHeight + 40
        radius: Theme.shape.large
        color: Theme.surfaceContainerHigh
        RowLayout {
            id: infoCol
            x: 20
            y: 20
            width: parent.width - 40
            spacing: 16
            Rectangle {
                Layout.alignment: Qt.AlignTop
                implicitWidth: 44
                implicitHeight: 44
                radius: 22
                color: Theme.secondaryContainer
                Icon { anchors.centerIn: parent; name: info.icon; size: 22; color: Theme.fgSecondaryContainer }
            }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 4
                Label { Layout.fillWidth: true; text: info.title; type: "titleMedium"; weight: 600 }
                Label {
                    Layout.fillWidth: true
                    text: info.text
                    type: "bodyMedium"
                    color: Theme.fgSurfaceVariant
                    wrapMode: Text.WordWrap
                    elide: Text.ElideNone
                }
                Button {
                    Layout.topMargin: 4
                    Layout.leftMargin: -12          // текстовая кнопка — вровень с текстом над ней
                    visible: info.action !== ""
                    text: info.action
                    style: "text"
                    onClicked: info.triggered()
                }
            }
        }
    }
}
