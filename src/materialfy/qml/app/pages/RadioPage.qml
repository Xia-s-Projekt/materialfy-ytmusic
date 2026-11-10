import QtQuick
import QtQuick.Layouts
import Md3
import Materialfy.Core
import ".."

// «Список треков» станции: те же 50 треков, что будут играть (Radio.tracks, контекст radio).
// «Пересобрать» — новый запрос станции, «Сохранить как плейлист» — плейлист из этих треков.
TrackListPage {
    id: page

    property string seed

    Component.onCompleted: if (seed !== "") Radio.open(seed)

    // обложка сида — из групп орбиты, иначе первого трека
    readonly property string seedCover: {
        for (const g of (Radio.seeds ?? []))
            for (const it of (g.items ?? []))
                if (it.uri === seed && it.cover)
                    return it.cover
        const t = model && model.count > 0 ? model.get(0) : null
        return t ? (t.coverLarge || t.cover || "") : ""
    }

    title: Radio.stationTitle !== "" ? Radio.stationTitle : I18n.t("Radio")
    overline: I18n.t("RADIO")
    heroImage: seedCover
    heroIcon: "radio"
    heroShape: "softSquare"
    subtitle: model && model.count > 0 ? I18n.t("{0} tracks · a fresh mix every time", model.count)
            : Radio.loading ? I18n.t("Building your station…") : ""
    model: Radio.tracks
    emptyIcon: "radio"
    emptyTitle: Radio.loading ? I18n.t("Building your station…") : Radio.errorText !== "" ? I18n.t("Couldn't build the station") : I18n.t("This station has no tracks")
    emptyText: Radio.errorText !== "" ? Radio.errorText
             : Radio.locked ? I18n.t("Stations come through a playback session: connect playback on this computer") : ""

    function notify(text) {
        const w = page.Window.window
        if (w && typeof w.notify === "function")
            w.notify(text, "", null)
    }

    headerExtra: Row {
        spacing: 4
        IconButton {
            icon: "refresh"
            size: "m"
            enabled: !Radio.loading
            onClicked: Radio.refresh()
        }
        IconButton {
            icon: "bookmark_add"
            size: "m"
            enabled: page.model !== null && page.model.count > 0
            onClicked: {
                Radio.save()
                page.notify(I18n.t("Saving station as a playlist…"))
            }
        }
    }
}
