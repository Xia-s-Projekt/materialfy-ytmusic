//! Home (get_home shelves) via ytmusicapi.

use std::sync::Arc;

use md3_model::HomeShelf;
use tokio::runtime::Handle;
use tokio::sync::watch;

use mf_platform::SettingsStore;
use yt_player::PlayerHandle;

use crate::auth::AuthService;
use crate::history::HistoryService;
use crate::library::LibraryService;
use crate::releases::ReleasesService;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct HomeSnapshot {
    pub loading: bool,
    pub shelves: Arc<Vec<HomeShelf>>,
    pub error_text: String,
}

pub struct HomeService {
    rt: Handle,
    auth: Arc<AuthService>,
    _player: PlayerHandle,
    _library: Arc<LibraryService>,
    _history: Arc<HistoryService>,
    _releases: Arc<ReleasesService>,
    _settings: Arc<SettingsStore>,
    snapshot: watch::Sender<HomeSnapshot>,
}

impl HomeService {
    pub fn new(
        rt: Handle,
        auth: Arc<AuthService>,
        player: PlayerHandle,
        library: Arc<LibraryService>,
        history: Arc<HistoryService>,
        releases: Arc<ReleasesService>,
        settings: Arc<SettingsStore>,
    ) -> Arc<Self> {
        let (snapshot, _) = watch::channel(HomeSnapshot::default());
        let service = Arc::new(Self { rt: rt.clone(), auth: auth.clone(), _player: player, _library: library, _history: history, _releases: releases, _settings: settings, snapshot });
        let s2 = service.clone();
        rt.spawn(async move { let _ = Self::load(&s2).await; });
        service
    }

    pub fn refresh(&self, _force: bool) {
        let rt = self.rt.clone();
        let api = self.auth.api();
        let snapshot = self.snapshot.clone();
        rt.spawn(async move {
            let _ = snapshot.send(HomeSnapshot { loading: true, ..Default::default() });
            match api.get_home(3).await {
                Ok(v) => {
                    let list: Vec<serde_json::Value> = v.as_array().cloned().unwrap_or_default();
                    let shelves = yt_api::map::home_shelves(&list);
                    let _ = snapshot.send(HomeSnapshot { loading: false, shelves: Arc::new(shelves), error_text: String::new() });
                }
                Err(e) => {
                    let _ = snapshot.send(HomeSnapshot { loading: false, error_text: e.user_text(), ..Default::default() });
                }
            }
        });
    }

    async fn load(service: &Arc<Self>) {
        let api = service.auth.api();
        match api.get_home(3).await {
            Ok(v) => {
                let list: Vec<serde_json::Value> = v.as_array().cloned().unwrap_or_default();
                let shelves = yt_api::map::home_shelves(&list);
                let _ = service.snapshot.send(HomeSnapshot { loading: false, shelves: Arc::new(shelves), error_text: String::new() });
            }
            Err(e) => {
                let _ = service.snapshot.send(HomeSnapshot { loading: false, error_text: e.user_text(), ..Default::default() });
            }
        }
    }

    pub fn watch(&self) -> watch::Receiver<HomeSnapshot> {
        self.snapshot.subscribe()
    }
    pub fn snapshot(&self) -> HomeSnapshot {
        self.snapshot.borrow().clone()
    }
}
