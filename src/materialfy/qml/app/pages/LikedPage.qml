import QtQuick
import Md3
import Materialfy.Core
import ".."

TrackListPage {
    title: I18n.t("Liked Songs")
    overline: I18n.t("COLLECTION")
    heroIcon: "favorite"
    heroShape: "softSquare"
    subtitle: Auth.state !== "signedIn" ? I18n.t("Tracks you’ve liked")
            : Library.liked.loading && Library.liked.count === 0 ? I18n.t("Loading…")
            : I18n.n(Library.liked.count, "{0} track", "{0} tracks")
    model: Auth.state === "signedIn" ? Library.liked : null
    // не загрузилось (сессия не ответила, Web API под 429) — ошибка и «Повторить», а не «пусто»
    readonly property bool failed: Library.errorText !== ""
    emptyIcon: failed ? "cloud_off" : "favorite"
    emptyTitle: failed ? I18n.t("Couldn’t load Liked Songs") : I18n.t("Nothing here yet")
    emptyText: failed ? Library.errorText : I18n.t("Tap the heart on tracks to see them here")
    emptyActionText: failed ? I18n.t("Retry") : ""
    emptyActionIcon: "refresh"
    onEmptyAction: Library.refresh()

    // Не вошли — приглашение войти
    EmptyState {
        anchors.centerIn: parent
        width: Math.min(parent.width - 64, 420)
        visible: Auth.state !== "signedIn"
        icon: "favorite"
        shape: "softSquare"
        title: I18n.t("Sign in to see your Liked Songs")
        text: I18n.t("Your saved tracks will sync from your YouTube Music account")
        actionText: I18n.t("Sign in")
        actionIcon: "login"
        onAction: parent.router.reset("account")
    }
}
