// Releases — a mirror of ReleasesService: "New releases radar" (radar: PlaylistItem keys, {} — not found),
// fresh artist releases (releases: AlbumItem keys + releaseDate, dateLabel, group).
use std::pin::Pin;
use std::sync::Arc;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use yt_services::ReleasesSnapshot;

use super::{forward_watch, to_qlist, to_qmap};
use crate::context::ctx;
use crate::qt::{QVariantList, QVariantMap};

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qlist.h");
        type QList_QVariant = cxx_qt_lib::QList<cxx_qt_lib::QVariant>;
        include!("cxx-qt-lib/qmap.h");
        type QMap_QString_QVariant = cxx_qt_lib::QMap<cxx_qt_lib::QMapPair_QString_QVariant>;
    }

    unsafe extern "RustQt" {
        #[qobject]
        // Lists get their own signal: a loading change does not rebuild the grid and rows
        #[qproperty(QMap_QString_QVariant, radar, READ, NOTIFY = radar_changed)]
        #[qproperty(bool, radar_checked, cxx_name = "radarChecked", READ, NOTIFY = radar_changed)]
        #[qproperty(QList_QVariant, releases, READ, NOTIFY = releases_changed)]
        #[qproperty(bool, loading, READ, NOTIFY = state_changed)]
        #[qproperty(QString, error_text, cxx_name = "errorText", READ, NOTIFY = state_changed)]
        type Releases = super::ReleasesRust;

        #[qsignal]
        #[cxx_name = "radarChanged"]
        fn radar_changed(self: Pin<&mut Releases>);
        #[qsignal]
        #[cxx_name = "releasesChanged"]
        fn releases_changed(self: Pin<&mut Releases>);
        #[qsignal]
        #[cxx_name = "stateChanged"]
        fn state_changed(self: Pin<&mut Releases>);

        /// Refresh (force — bypassing the 6-hour limit; without force — only if the data is stale).
        #[qinvokable]
        fn refresh(self: &Releases, force: bool);
    }

    impl cxx_qt::Threading for Releases {}
    impl cxx_qt::Initialize for Releases {}
}

#[derive(Default)]
pub struct ReleasesRust {
    snapshot: ReleasesSnapshot,
    radar: QVariantMap,
    radar_checked: bool,
    releases: QVariantList,
    loading: bool,
    error_text: QString,
}

impl qobject::Releases {
    fn refresh(&self, force: bool) {
        ctx().services.releases.refresh(force);
    }

    fn apply(mut self: Pin<&mut Self>, next: ReleasesSnapshot) {
        let prev = std::mem::replace(&mut self.as_mut().rust_mut().snapshot, next.clone());
        if prev == next {
            return;
        }
        let radar = prev.radar != next.radar || prev.radar_checked != next.radar_checked;
        let releases = !Arc::ptr_eq(&prev.releases, &next.releases) && prev.releases != next.releases;
        let state = prev.loading != next.loading || prev.error_text != next.error_text;
        let mut this = self.as_mut().rust_mut();
        if radar {
            this.radar = next.radar.as_ref().map(to_qmap).unwrap_or_default();
            this.radar_checked = next.radar_checked;
        }
        if releases {
            this.releases = to_qlist(&next.releases);
        }
        this.loading = next.loading;
        this.error_text = QString::from(next.error_text.as_str());
        if radar {
            self.as_mut().radar_changed();
        }
        if releases {
            self.as_mut().releases_changed();
        }
        if state {
            self.as_mut().state_changed();
        }
    }
}

impl cxx_qt::Initialize for qobject::Releases {
    fn initialize(mut self: Pin<&mut Self>) {
        let mut watch = ctx().services.releases.watch();
        let snapshot = watch.borrow_and_update().clone();
        // the first snapshot is published in full (prev == default would cut off an empty one)
        let mut this = self.as_mut().rust_mut();
        this.radar = snapshot.radar.as_ref().map(to_qmap).unwrap_or_default();
        this.radar_checked = snapshot.radar_checked;
        this.releases = to_qlist(&snapshot.releases);
        this.loading = snapshot.loading;
        this.error_text = QString::from(snapshot.error_text.as_str());
        this.snapshot = snapshot;
        forward_watch(self.qt_thread(), watch, Self::apply);
    }
}
