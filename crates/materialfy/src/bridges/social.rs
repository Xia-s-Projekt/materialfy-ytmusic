// Social — mirror of SocialService: friend activity (friends: FriendActivity keys) and the Jam the account is in
// (jam: Jam keys with members; {} — none).
use std::pin::Pin;
use std::sync::Arc;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use yt_services::SocialSnapshot;

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
        #[qproperty(QList_QVariant, friends, READ, NOTIFY = friends_changed)]
        #[qproperty(bool, friends_loaded, cxx_name = "friendsLoaded", READ, NOTIFY = friends_state_changed)]
        #[qproperty(bool, friends_loading, cxx_name = "friendsLoading", READ, NOTIFY = friends_state_changed)]
        #[qproperty(QString, friends_error, cxx_name = "friendsError", READ, NOTIFY = friends_state_changed)]
        #[qproperty(QMap_QString_QVariant, jam, READ, NOTIFY = jam_changed)]
        #[qproperty(bool, in_jam, cxx_name = "inJam", READ, NOTIFY = jam_changed)]
        #[qproperty(bool, jam_checked, cxx_name = "jamChecked", READ, NOTIFY = jam_state_changed)]
        #[qproperty(bool, jam_busy, cxx_name = "jamBusy", READ, NOTIFY = jam_state_changed)]
        #[qproperty(QString, jam_error, cxx_name = "jamError", READ, NOTIFY = jam_state_changed)]
        type Social = super::SocialRust;

        #[qsignal]
        #[cxx_name = "friendsChanged"]
        fn friends_changed(self: Pin<&mut Social>);
        #[qsignal]
        #[cxx_name = "friendsStateChanged"]
        fn friends_state_changed(self: Pin<&mut Social>);
        #[qsignal]
        #[cxx_name = "jamChanged"]
        fn jam_changed(self: Pin<&mut Social>);
        #[qsignal]
        #[cxx_name = "jamStateChanged"]
        fn jam_state_changed(self: Pin<&mut Social>);

        /// Re-read friend activity (force — even if read moments ago).
        #[qinvokable]
        #[cxx_name = "refreshFriends"]
        fn refresh_friends(self: &Social, force: bool);
        #[qinvokable]
        #[cxx_name = "refreshJam"]
        fn refresh_jam(self: &Social);
        /// Start a Jam hosted by this computer.
        #[qinvokable]
        #[cxx_name = "startJam"]
        fn start_jam(self: &Social);
        /// Join by an invite link or code.
        #[qinvokable]
        #[cxx_name = "joinJam"]
        fn join_jam(self: &Social, link: &QString);
        /// Leave the Jam (the host ends it).
        #[qinvokable]
        #[cxx_name = "leaveJam"]
        fn leave_jam(self: &Social);
        /// Copy the invite link; false — no Jam.
        #[qinvokable]
        #[cxx_name = "copyJamLink"]
        fn copy_jam_link(self: &Social) -> bool;
    }

    impl cxx_qt::Threading for Social {}
    impl cxx_qt::Initialize for Social {}
}

#[derive(Default)]
pub struct SocialRust {
    snapshot: SocialSnapshot,
    friends: QVariantList,
    friends_loaded: bool,
    friends_loading: bool,
    friends_error: QString,
    jam: QVariantMap,
    in_jam: bool,
    jam_checked: bool,
    jam_busy: bool,
    jam_error: QString,
}

impl qobject::Social {
    fn refresh_friends(&self, force: bool) {
        ctx().services.social.refresh_friends(force);
    }

    fn refresh_jam(&self) {
        ctx().services.social.refresh_jam();
    }

    fn start_jam(&self) {
        ctx().services.social.start_jam();
    }

    fn join_jam(&self, link: &QString) {
        ctx().services.social.join_jam(&link.to_string());
    }

    fn leave_jam(&self) {
        ctx().services.social.leave_jam();
    }

    fn copy_jam_link(&self) -> bool {
        let link = self.rust().snapshot.jam.as_ref().map(|jam| jam.share_url.clone()).unwrap_or_default();
        if link.is_empty() {
            return false;
        }
        crate::qt::copy_to_clipboard(&link);
        true
    }

    fn apply(mut self: Pin<&mut Self>, next: SocialSnapshot) {
        let prev = std::mem::replace(&mut self.as_mut().rust_mut().snapshot, next.clone());
        if prev == next {
            return;
        }
        let friends = !Arc::ptr_eq(&prev.friends, &next.friends) && prev.friends != next.friends;
        let friends_state = prev.friends_loaded != next.friends_loaded
            || prev.friends_loading != next.friends_loading
            || prev.friends_error != next.friends_error;
        let jam = prev.jam != next.jam;
        let jam_state =
            prev.jam_checked != next.jam_checked || prev.jam_busy != next.jam_busy || prev.jam_error != next.jam_error;
        let mut this = self.as_mut().rust_mut();
        if friends {
            this.friends = to_qlist(&next.friends);
        }
        if jam {
            this.jam = next.jam.as_ref().map(to_qmap).unwrap_or_default();
            this.in_jam = next.jam.is_some();
        }
        Self::copy_state(&mut this, &next);
        if friends {
            self.as_mut().friends_changed();
        }
        if friends_state {
            self.as_mut().friends_state_changed();
        }
        if jam {
            self.as_mut().jam_changed();
        }
        if jam_state {
            self.as_mut().jam_state_changed();
        }
    }

    fn copy_state(this: &mut SocialRust, snapshot: &SocialSnapshot) {
        this.friends_loaded = snapshot.friends_loaded;
        this.friends_loading = snapshot.friends_loading;
        this.friends_error = QString::from(snapshot.friends_error.as_str());
        this.jam_checked = snapshot.jam_checked;
        this.jam_busy = snapshot.jam_busy;
        this.jam_error = QString::from(snapshot.jam_error.as_str());
    }
}

impl cxx_qt::Initialize for qobject::Social {
    fn initialize(mut self: Pin<&mut Self>) {
        let mut watch = ctx().services.social.watch();
        let snapshot = watch.borrow_and_update().clone();
        let mut this = self.as_mut().rust_mut();
        this.friends = to_qlist(&snapshot.friends);
        this.jam = snapshot.jam.as_ref().map(to_qmap).unwrap_or_default();
        this.in_jam = snapshot.jam.is_some();
        Self::copy_state(&mut this, &snapshot);
        this.snapshot = snapshot;
        forward_watch(self.qt_thread(), watch, Self::apply);
    }
}
