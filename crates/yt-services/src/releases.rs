//! New releases: stub (YTMusic has no "radar" playlist; use search later).

use std::sync::Arc;

use serde::Serialize;

use md3_model::PlaylistItem;
use tokio::runtime::Handle;
use tokio::sync::watch;

use crate::auth::AuthService;
use crate::history::HistoryService;
use crate::library::LibraryService;
use yt_player::PlayerHandle;

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NewRelease {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub cover: String,
    pub date: String,
    pub date_label: String,
    pub group: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ReleasesSnapshot {
    pub loading: bool,
    pub radar: Option<PlaylistItem>,
    pub radar_checked: bool,
    pub releases: Arc<Vec<NewRelease>>,
    pub error_text: String,
}

pub struct ReleasesService {
    _rt: Handle,
    _auth: Arc<AuthService>,
    _library: Arc<LibraryService>,
    _history: Arc<HistoryService>,
    _player: PlayerHandle,
    snapshot: watch::Sender<ReleasesSnapshot>,
}

impl ReleasesService {
    pub fn new(
        rt: Handle,
        auth: Arc<AuthService>,
        library: Arc<LibraryService>,
        history: Arc<HistoryService>,
        player: PlayerHandle,
    ) -> Arc<Self> {
        let (snapshot, _) = watch::channel(ReleasesSnapshot { radar_checked: true, ..Default::default() });
        Arc::new(Self { _rt: rt, _auth: auth, _library: library, _history: history, _player: player, snapshot })
    }
    pub fn refresh(&self, _force: bool) {}
    pub fn snapshot(&self) -> ReleasesSnapshot { self.snapshot.borrow().clone() }
    pub fn watch(&self) -> watch::Receiver<ReleasesSnapshot> { self.snapshot.subscribe() }
}
