//! Application services (without Qt) — a facade of the same shape as the yamusic-md3 + devices, history, home services.
//! Every service is an Arc; the state is a watch (snapshot) and/or a broadcast (events). QML bridges subscribe in
//! cxx_qt::Initialize and move changes into the GUI thread through CxxQtThread::queue.
//!
//! One tokio runtime for everything (the shell creates it, engine.rs): services, the PlayerHandle actor, librespot
//! (Session/Spirc spawn tasks on the current runtime) and the bridge tasks. Blocking work (keyring, zeroconf,
//! cache sizes) — spawn_blocking/own threads. Sign-in does NOT start in start — the shell calls auth.start()
//! after QML loads (unless --offline).

pub mod auth;
pub mod catalog;
pub mod cover_fallback;
/// internal "services-core"
pub mod data;
pub mod devices;
/// internal "services-extra"
pub mod extra;
pub mod glue;
pub mod history;
pub mod home;
pub mod library;
pub mod lyrics;
/// "New releases": "Release Radar" and fresh releases by artists
pub mod releases;
/// Friends (friend activity) and Jam through the session
pub mod social;
/// auto-update from GitHub Releases
pub mod update;
pub mod wave;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use sf_api::ApiGateway;
use sf_player::{PlayerConfig, PlayerHandle};
use tokio::runtime::Handle;
use tokio::sync::broadcast;
use mf_platform::{SettingKey, SettingsStore};

pub use auth::{AuthEvent, AuthService, AuthSnapshot, AuthState, PlaybackAuthState, ProxyInput};
pub use sf_auth::{Grant, ManualProxy, ProxyConfig, ProxyMode};
pub use catalog::{AlbumPage, ArtistPage, CatalogService, EpisodePage, SearchKind, SearchResults, ShowPage};
pub use devices::{DevicesEvent, DevicesService, DevicesSnapshot};
pub use history::{HistoryEvent, HistoryService, PlayRecord};
pub use home::{HomeService, HomeSnapshot};
pub use library::{FeedbackKind, LibraryEvent, LibraryKind, LibraryService, PlaylistInfo};
pub use lyrics::{LyricsService, LyricsSnapshot, LyricsStatus};
pub use md3_model::{ListChange, PlaylistId};
pub use releases::{NewRelease, ReleasesService, ReleasesSnapshot};
pub use social::{FriendActivity, Jam, JamMember, SocialService, SocialSnapshot};
pub use update::{UpdateService, UpdateSnapshot, UpdateState};
pub use wave::{WaveService, WaveSnapshot};

#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    #[error("{}", mf_i18n::t("Sign in to your account"))]
    NotSignedIn,
    #[error("{}", data::route::api_error_text(.0))]
    Api(#[from] sf_api::ApiError),
    #[error("{0}")]
    Other(String),
}

/// Requests to the Qt shell (media keys/MPRIS Raise/Quit).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppRequest {
    RaiseWindow,
    Quit,
}

#[derive(Clone, Debug, Default)]
pub struct ServicesOptions {
    /// save/restore the manual queue and the resume point (false in snapshot mode)
    pub persist_session: bool,
    /// media keys/SMTC/MPRIS (false in snapshot mode)
    pub enable_media: bool,
}

/// All services. Clone is cheap (Arc).
#[derive(Clone)]
pub struct Services {
    pub settings: Arc<SettingsStore>,
    pub api: Arc<ApiGateway>,
    pub auth: Arc<AuthService>,
    pub library: Arc<LibraryService>,
    pub catalog: Arc<CatalogService>,
    /// Spotify radio (formerly "My Wave")
    pub wave: Arc<WaveService>,
    pub home: Arc<HomeService>,
    /// "New releases"
    pub releases: Arc<ReleasesService>,
    pub lyrics: Arc<LyricsService>,
    pub history: Arc<HistoryService>,
    pub devices: Arc<DevicesService>,
    /// friend activity and Jam
    pub social: Arc<SocialService>,
    /// auto-update (checks start from the Updater bridge, not here)
    pub update: Arc<update::UpdateService>,
    pub player: PlayerHandle,
    app_requests: broadcast::Sender<AppRequest>,
    media: Arc<Mutex<Option<glue::MediaStop>>>,
}

/// The "Covers bypass proxy" setting → the cover client's flag (changes at once, without a restart).
fn follow_covers_bypass(rt: &Handle, settings: &Arc<SettingsStore>, direct: Arc<std::sync::atomic::AtomicBool>) {
    let settings = settings.clone();
    let mut changes = settings.subscribe();
    rt.spawn(async move {
        loop {
            match changes.recv().await {
                Ok(SettingKey::CoversBypassProxy) | Err(broadcast::error::RecvError::Lagged(_)) => {
                    direct.store(settings.get_bool(SettingKey::CoversBypassProxy), std::sync::atomic::Ordering::Relaxed);
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

impl Services {
    /// Order: HTTP (proxy from settings) → ApiGateway → PlayerHandle → Auth → History → Library → Catalog →
    /// Wave → Releases → Home → Lyrics → Devices → glue (settings → player) → media keys.
    pub fn start(rt: &Handle, settings: Arc<SettingsStore>, options: ServicesOptions) -> anyhow::Result<Services> {
        // the applied proxy from settings; auth.start() will supply the password from the keyring
        let proxy = auth::settings_proxy(&settings, None);
        let http = sf_api::Http::from_proxy(&proxy).unwrap_or_else(|error| {
            tracing::warn!("invalid proxy settings: {error:#}");
            sf_api::Http::unavailable(error)
        });
        let api = Arc::new(ApiGateway::new(http.clone(), Arc::new(sf_api::NetActivity::default())));
        let covers_direct = Arc::new(std::sync::atomic::AtomicBool::new(settings.get_bool(SettingKey::CoversBypassProxy)));
        follow_covers_bypass(rt, &settings, covers_direct.clone());
        let config = PlayerConfig {
            cover_dir: mf_platform::dirs::cache_subdir("covers"),
            session_file: options.persist_session.then(mf_platform::dirs::session_file),
            volume: settings.get_f64(SettingKey::Volume).clamp(0.0, 1.0),
            // a client for covers only: per the "Covers bypass proxy" setting — direct
            http: http.bypassing(covers_direct),
        };
        let player = PlayerHandle::start(rt, config, api.clone()).map_err(|e| anyhow::anyhow!("player: {e}"))?;
        let store = sf_auth::Store::with_runtime(mf_platform::dirs::state_dir(), rt.clone());
        let auth = AuthService::new(rt.clone(), settings.clone(), api.clone(), http.clone(), player.clone(), store);
        let history = HistoryService::new(rt.clone(), auth.clone(), player.clone());
        let library =
            LibraryService::new(rt.clone(), auth.clone(), player.clone(), history.clone(), settings.clone());
        let catalog = CatalogService::new(auth.clone(), player.clone());
        let wave = WaveService::new(
            rt.clone(),
            auth.clone(),
            player.clone(),
            library.clone(),
            history.clone(),
            settings.clone(),
        );
        let releases =
            ReleasesService::new(rt.clone(), auth.clone(), library.clone(), history.clone(), player.clone(), http.clone());
        let home = HomeService::new(
            rt.clone(),
            auth.clone(),
            player.clone(),
            library.clone(),
            history.clone(),
            releases.clone(),
            settings.clone(),
        );
        // texts — through the shared client: the same proxy as the Web API, changes apply at once
        let fetcher = {
            let http = http.clone();
            mf_lyrics::LyricsFetcher::with_source(mf_platform::dirs::cache_subdir("lyrics"), move || http.client())
        };
        let lyrics = LyricsService::new(rt.clone(), player.clone(), fetcher);
        let devices = DevicesService::new(rt.clone(), auth.clone(), player.clone(), settings.clone());
        let social = SocialService::new(rt.clone(), auth.clone(), player.clone());
        glue::spawn_settings_glue(rt, settings.clone(), player.clone());

        let (app_requests, _) = broadcast::channel(16);
        let update = update::UpdateService::new(rt.clone(), settings.clone(), http.clone(), app_requests.clone());
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
            social,
            update,
            player,
            app_requests,
            media: Arc::new(Mutex::new(media)),
        })
    }

    pub fn app_requests(&self) -> broadcast::Receiver<AppRequest> {
        self.app_requests.subscribe()
    }

    /// On exit (from the GUI thread after exec): save the session and history, stop the engine and the media keys.
    /// Blocks for ≤ 2 s.
    pub fn shutdown(&self) {
        self.player.shutdown();
        self.history.flush();
        if let Some(stop) = lock(&self.media).take() {
            stop.stop(Duration::from_millis(500));
        }
    }
}

/// A Mutex that does not panic on "poisoning".
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
