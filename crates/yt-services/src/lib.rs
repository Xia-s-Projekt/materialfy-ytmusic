//! Services façade for Materialfy-YT (fork of fatyzzz/materialfy).
//! Replaces sf-services (Spotify) with YouTube Music (ytmusicapi) services.
//! Public API mirrors sf-services so QML bridges need only `s/sf_services/yt_services/`.

pub mod auth;
pub mod catalog;
pub mod cover_fallback;
pub mod data;
pub mod devices;
pub mod glue;
pub mod history;
pub mod home;
pub mod library;
pub mod lyrics;
pub mod releases;
pub mod update;
pub mod wave;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::runtime::Handle;
use tokio::sync::broadcast;
use mf_platform::{SettingKey, SettingsStore};

use yt_api::YtClient;
use yt_player::{PlayerConfig, PlayerHandle};

pub use auth::{AuthEvent, AuthService, AuthSnapshot, AuthState, Grant, PlaybackAuthState, ProxyInput};
pub use yt_auth::{HeadersAuth, Store as AuthStore};
pub use yt_auth::store::SavedAccount;
pub use yt_api::{YtError, client::YtClient as YtApiClient};
pub use catalog::{AlbumPage, ArtistPage, CatalogService, EpisodePage, SearchKind, SearchResults, ShowPage};
pub use devices::{DevicesEvent, DevicesService, DevicesSnapshot};
pub use history::{HistoryEvent, HistoryService, PlayRecord};
pub use home::{HomeService, HomeSnapshot};
pub use library::{FeedbackKind, LibraryEvent, LibraryKind, LibraryService, PlaylistInfo};
pub use lyrics::{LyricsService, LyricsSnapshot, LyricsStatus};
pub use releases::{NewRelease, ReleasesService, ReleasesSnapshot};
pub use update::{UpdateService, UpdateSnapshot, UpdateState};
pub use wave::{WaveService, WaveSnapshot};

/// Proxy policy matching sf-auth::ProxyConfig so bridges compile unchanged.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ProxyConfig {
    Invalid(String),
    Off,
    #[default]
    System,
    Http(ManualProxy),
    Socks(ManualProxy),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ManualProxy {
    host: String,
    port: u16,
    username: String,
    password: String,
}
impl ManualProxy {
    pub fn host(&self) -> &str { &self.host }
    pub fn port(&self) -> u16 { self.port }
    pub fn username(&self) -> &str { &self.username }
    pub fn password(&self) -> &str { &self.password }
}

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("Sign in to your account")]
    NotSignedIn,
    #[error("{0}")]
    Api(#[from] yt_api::YtError),
    #[error("{0}")]
    Other(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppRequest {
    RaiseWindow,
    Quit,
}

#[derive(Clone, Debug, Default)]
pub struct ServicesOptions {
    pub persist_session: bool,
    pub enable_media: bool,
}

#[derive(Clone)]
pub struct Services {
    pub settings: Arc<SettingsStore>,
    pub api: Arc<YtClient>,
    pub auth: Arc<AuthService>,
    pub library: Arc<LibraryService>,
    pub catalog: Arc<CatalogService>,
    pub wave: Arc<WaveService>,
    pub home: Arc<HomeService>,
    pub releases: Arc<ReleasesService>,
    pub lyrics: Arc<LyricsService>,
    pub history: Arc<HistoryService>,
    pub devices: Arc<DevicesService>,
    pub update: Arc<update::UpdateService>,
    pub player: PlayerHandle,
    app_requests: broadcast::Sender<AppRequest>,
    media: Arc<Mutex<Option<glue::MediaStop>>>,
}

impl Services {
    pub fn start(rt: &Handle, settings: Arc<SettingsStore>, options: ServicesOptions) -> anyhow::Result<Services> {
        let store = yt_auth::Store::new(mf_platform::dirs::state_dir());
        let headers = store.file_path();
        let api = Arc::new(YtClient::new(Some(headers)));
        let config = PlayerConfig {
            cover_dir: mf_platform::dirs::cache_subdir("covers"),
            session_file: options.persist_session.then(mf_platform::dirs::session_file),
            volume: settings.get_f64(SettingKey::Volume).clamp(0.0, 1.0),
            http: reqwest::Client::new(),
        };
        let player = PlayerHandle::start(rt, config, api.clone())
            .map_err(|e| anyhow::anyhow!("player: {e}"))?;
        let auth = AuthService::new(rt.clone(), settings.clone(), api.clone(), player.clone(), store);
        let history = HistoryService::new(rt.clone(), auth.clone(), player.clone());
        let library = LibraryService::new(rt.clone(), auth.clone(), player.clone(), history.clone(), settings.clone());
        let catalog = CatalogService::new(auth.clone(), library.clone(), player.clone());
        let wave = WaveService::new(rt.clone(), auth.clone(), player.clone(), library.clone(), history.clone(), settings.clone());
        let releases = ReleasesService::new(rt.clone(), auth.clone(), library.clone(), history.clone(), player.clone());
        let home = HomeService::new(rt.clone(), auth.clone(), player.clone(), library.clone(), history.clone(), releases.clone(), settings.clone());
        let fetcher = mf_lyrics::LyricsFetcher::new(mf_platform::dirs::cache_subdir("lyrics"));
        let lyrics = LyricsService::new(rt.clone(), player.clone(), fetcher, api.clone());
        let devices = DevicesService::new(rt.clone(), auth.clone(), player.clone(), settings.clone());
        glue::spawn_settings_glue(rt, settings.clone(), player.clone());
        let (app_requests, _) = broadcast::channel(16);
        let update = update::UpdateService::new(rt.clone(), settings.clone(), app_requests.clone());
        let media = if options.enable_media {
            glue::start_media_controls(rt, settings.clone(), player.clone(), app_requests.clone())
        } else {
            None
        };
        Ok(Services {
            settings,
            api,
            auth,
            library,
            catalog,
            wave,
            home,
            releases,
            lyrics,
            history,
            devices,
            update,
            player,
            app_requests,
            media: Arc::new(Mutex::new(media)),
        })
    }

    pub fn app_requests(&self) -> broadcast::Receiver<AppRequest> {
        self.app_requests.subscribe()
    }

    pub fn shutdown(&self) {
        self.player.shutdown();
        self.history.flush();
        if let Some(stop) = lock(&self.media).take() {
            stop.stop(Duration::from_millis(500));
        }
    }
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
