// Home - a mirror of HomeService: home shelves ({key, title, kind, items:[...]}).
use std::pin::Pin;
use std::sync::Arc;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use yt_services::HomeSnapshot;

use super::{forward_watch, to_qlist};
use crate::context::ctx;
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
        // A separate signal for each property: a loading/errorText change should not rebuild the shelves in QML
        #[qproperty(QList_QVariant, shelves, READ, NOTIFY = shelves_changed)]
        #[qproperty(bool, loading, READ, NOTIFY = loading_changed)]
        #[qproperty(QString, error_text, cxx_name = "errorText", READ, NOTIFY = error_text_changed)]
        type Home = super::HomeRust;

        #[qsignal]
        #[cxx_name = "shelvesChanged"]
        fn shelves_changed(self: Pin<&mut Home>);
        #[qsignal]
        #[cxx_name = "loadingChanged"]
        fn loading_changed(self: Pin<&mut Home>);
        #[qsignal]
        #[cxx_name = "errorTextChanged"]
        fn error_text_changed(self: Pin<&mut Home>);

        /// Refresh the shelves (force - without the 10-minute limit).
        #[qinvokable]
        fn refresh(self: &Home, force: bool);
    }

    impl cxx_qt::Threading for Home {}
    impl cxx_qt::Initialize for Home {}
}

#[derive(Default)]
pub struct HomeRust {
    snapshot: HomeSnapshot,
    shelves: QVariantList,
    loading: bool,
    error_text: QString,
}

impl qobject::Home {
    fn refresh(&self, force: bool) {
        ctx().services.home.refresh(force);
    }

    fn apply(mut self: Pin<&mut Self>, next: HomeSnapshot) {
        let prev = std::mem::replace(&mut self.as_mut().rust_mut().snapshot, next.clone());
        if prev == next {
            return;
        }
        // shelves are compared by content: the service may send a new Arc with the same shelves
        let shelves = !Arc::ptr_eq(&prev.shelves, &next.shelves) && prev.shelves != next.shelves;
        let loading = prev.loading != next.loading;
        let error = prev.error_text != next.error_text;
        let mut this = self.as_mut().rust_mut();
        if shelves {
            this.shelves = to_qlist(&next.shelves);
        }
        this.loading = next.loading;
        if error {
            this.error_text = QString::from(next.error_text.as_str());
        }
        if shelves {
            self.as_mut().shelves_changed();
        }
        if loading {
            self.as_mut().loading_changed();
        }
        if error {
            self.as_mut().error_text_changed();
        }
    }
}

impl cxx_qt::Initialize for qobject::Home {
    fn initialize(mut self: Pin<&mut Self>) {
        let mut watch = ctx().services.home.watch();
        let snapshot = watch.borrow_and_update().clone();
        self.as_mut().apply(snapshot);
        forward_watch(self.qt_thread(), watch, Self::apply);
    }
}
