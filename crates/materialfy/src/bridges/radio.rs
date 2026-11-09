// Radio — зеркало WaveService (радио Spotify, бывшая «Моя волна»): свойства из снимка, сигнал — по группе.
use std::pin::Pin;
use std::sync::Arc;

use cxx::UniquePtr;
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use md3_model::{PlayContext, contexts};
use yt_services::WaveSnapshot;

use super::{forward_watch, model_ptr, to_qlist, with_model};
use crate::context::ctx;
use crate::models::{DictListModel, ModelKind, to_rows};
use crate::qt::QVariantList;

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qlist.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qproperty(bool, active, READ, NOTIFY = state_changed)]
        #[qproperty(bool, loading, READ, NOTIFY = state_changed)]
        #[qproperty(bool, locked, READ, NOTIFY = state_changed)]
        #[qproperty(QString, station_title, cxx_name = "stationTitle", READ, NOTIFY = state_changed)]
        #[qproperty(QString, station_uri, cxx_name = "stationUri", READ, NOTIFY = state_changed)]
        #[qproperty(QString, selected, READ, NOTIFY = state_changed)]
        #[qproperty(QList_QVariant, seeds, READ, NOTIFY = seeds_changed)]
        #[qproperty(*mut QObject, tracks, READ = tracks_ptr, CONSTANT)]
        #[qproperty(QString, error_text, cxx_name = "errorText", READ, NOTIFY = error_changed)]
        type Radio = super::RadioRust;

        fn tracks_ptr(self: &Radio) -> *mut QObject;

        #[qsignal]
        #[cxx_name = "stateChanged"]
        fn state_changed(self: Pin<&mut Radio>);
        #[qsignal]
        #[cxx_name = "seedsChanged"]
        fn seeds_changed(self: Pin<&mut Radio>);
        #[qsignal]
        #[cxx_name = "errorChanged"]
        fn error_changed(self: Pin<&mut Radio>);

        /// Большая кнопка: станция выбранного сида или пауза/продолжение.
        #[qinvokable]
        fn play(self: &Radio);
        /// Радио от сида: "artist:ID" | "track:ID" | "album:ID" | "playlist:ID" | spotify:…-URI.
        #[qinvokable]
        #[cxx_name = "playStation"]
        fn play_station(self: &Radio, seed: &QString, title: &QString);
        /// Выбрать сид на орбите (без запуска).
        #[qinvokable]
        fn select(self: &Radio, seed_uri: &QString);
        /// Открыть станцию (треки без воспроизведения).
        #[qinvokable]
        fn open(self: &Radio, seed_uri: &QString);
        /// «Пересобрать» — новый микс.
        #[qinvokable]
        fn refresh(self: &Radio);
        /// «Сохранить как плейлист».
        #[qinvokable]
        fn save(self: &Radio);
    }

    impl cxx_qt::Threading for Radio {}
    impl cxx_qt::Initialize for Radio {}
}

pub struct RadioRust {
    snapshot: WaveSnapshot,
    active: bool,
    loading: bool,
    locked: bool,
    station_title: QString,
    station_uri: QString,
    selected: QString,
    seeds: QVariantList,
    tracks: UniquePtr<DictListModel>,
    error_text: QString,
}

impl Default for RadioRust {
    fn default() -> Self {
        let mut tracks = DictListModel::create(ModelKind::Track);
        with_model(&mut tracks, |m| m.set_context(PlayContext::new(contexts::RADIO)));
        Self {
            snapshot: WaveSnapshot::default(),
            active: false,
            loading: false,
            locked: false,
            station_title: QString::default(),
            station_uri: QString::default(),
            selected: QString::default(),
            seeds: QVariantList::default(),
            tracks,
            error_text: QString::default(),
        }
    }
}

/// Сид станции: плитки «Радио» на главной несут и URI станции (spotify:station:…) — он сводится к сиду.
fn seed_of(text: &QString) -> String {
    let text = text.to_string();
    yt_api::util::station_seed(&text).unwrap_or(text)
}

impl qobject::Radio {
    fn tracks_ptr(&self) -> *mut cxx_qt::QObject {
        model_ptr(&self.tracks)
    }

    fn play(&self) {
        ctx().services.wave.play();
    }

    fn play_station(&self, seed: &QString, title: &QString) {
        ctx().services.wave.play_station(seed_of(seed), title.to_string());
    }

    fn select(&self, seed_uri: &QString) {
        ctx().services.wave.select(seed_of(seed_uri));
    }

    fn open(&self, seed_uri: &QString) {
        ctx().services.wave.open(seed_of(seed_uri));
    }

    fn refresh(&self) {
        ctx().services.wave.refresh();
    }

    fn save(&self) {
        ctx().services.wave.save();
    }

    fn apply(mut self: Pin<&mut Self>, next: WaveSnapshot) {
        let prev = std::mem::replace(&mut self.as_mut().rust_mut().snapshot, next.clone());
        if prev == next {
            return;
        }
        if !Arc::ptr_eq(&prev.seeds, &next.seeds) && prev.seeds != next.seeds {
            self.as_mut().rust_mut().seeds = to_qlist(&next.seeds);
            self.as_mut().seeds_changed();
        }
        if !Arc::ptr_eq(&prev.tracks, &next.tracks) && prev.tracks != next.tracks {
            let station = next.station_uri.clone();
            let title = next.station_title.clone();
            with_model(&mut self.as_mut().rust_mut().get_mut().tracks, |mut m| {
                m.as_mut().set_context(PlayContext::with_uri(contexts::RADIO, station).titled(title));
                m.reset_rows(to_rows(&next.tracks));
            });
        }
        if prev.active != next.active
            || prev.loading != next.loading
            || prev.locked != next.locked
            || prev.station_title != next.station_title
            || prev.station_uri != next.station_uri
            || prev.selected != next.selected
        {
            let mut this = self.as_mut().rust_mut();
            this.active = next.active;
            this.loading = next.loading;
            this.locked = next.locked;
            this.station_title = QString::from(next.station_title.as_str());
            this.station_uri = QString::from(next.station_uri.as_str());
            this.selected = QString::from(next.selected.as_str());
            self.as_mut().state_changed();
        }
        if prev.error_text != next.error_text {
            self.as_mut().rust_mut().error_text = QString::from(next.error_text.as_str());
            self.as_mut().error_changed();
        }
    }
}

impl cxx_qt::Initialize for qobject::Radio {
    fn initialize(mut self: Pin<&mut Self>) {
        let mut watch = ctx().services.wave.watch();
        let snapshot = watch.borrow_and_update().clone();
        self.as_mut().apply(snapshot);
        forward_watch(self.qt_thread(), watch, Self::apply);
    }
}
