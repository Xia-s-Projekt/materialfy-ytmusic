// Lyrics (lyrics.py) — зеркало LyricsService: одно changed() на каждое изменение снимка.
use std::pin::Pin;
use std::sync::Arc;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use yt_services::LyricsSnapshot;

use super::forward_watch;
use crate::context::ctx;
use crate::qt::{QVariantList, json_to_qvariantlist};

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
        #[qproperty(QList_QVariant, lines, READ, NOTIFY = changed)]
        #[qproperty(QString, track_id, cxx_name = "trackId", READ, NOTIFY = changed)]
        #[qproperty(QString, status, READ, NOTIFY = changed)]
        #[qproperty(QString, source, READ, NOTIFY = changed)]
        #[qproperty(QString, kind, READ, NOTIFY = changed)]
        type Lyrics = super::LyricsRust;

        #[qsignal]
        fn changed(self: Pin<&mut Lyrics>);
    }

    impl cxx_qt::Threading for Lyrics {}
    impl cxx_qt::Initialize for Lyrics {}
}

pub struct LyricsRust {
    snapshot: LyricsSnapshot,
    lines: QVariantList,
    track_id: QString,
    status: QString,
    source: QString,
    kind: QString,
}

impl Default for LyricsRust {
    fn default() -> Self {
        let snapshot = LyricsSnapshot::default();
        Self {
            status: QString::from(snapshot.status.as_str()),
            snapshot,
            lines: QVariantList::default(),
            track_id: QString::default(),
            source: QString::default(),
            kind: QString::default(),
        }
    }
}

impl qobject::Lyrics {
    fn apply(mut self: Pin<&mut Self>, next: LyricsSnapshot) {
        if self.snapshot == next {
            return;
        }
        let mut this = self.as_mut().rust_mut();
        if !Arc::ptr_eq(&this.snapshot.lines, &next.lines) {
            this.lines = match serde_json::to_value(&*next.lines) {
                Ok(serde_json::Value::Array(items)) => json_to_qvariantlist(&items),
                _ => QVariantList::default(),
            };
        }
        this.track_id = QString::from(next.track_id.as_str());
        this.status = QString::from(next.status.as_str());
        this.source = QString::from(next.source.as_str());
        this.kind = QString::from(next.kind.as_str());
        this.snapshot = next;
        self.changed();
    }
}

impl cxx_qt::Initialize for qobject::Lyrics {
    fn initialize(mut self: Pin<&mut Self>) {
        let mut watch = ctx().services.lyrics.watch();
        let snapshot = watch.borrow_and_update().clone();
        self.as_mut().apply(snapshot);
        forward_watch(self.qt_thread(), watch, Self::apply);
    }
}
