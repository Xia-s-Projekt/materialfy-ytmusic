//! Auth service for YTMusic (replaces Spotify PKCE).
//! Sign-in = paste ytmusicapi headers-auth JSON. No separate playback grant.

use std::sync::Arc;

use tokio::runtime::Handle;
use tokio::sync::{broadcast, watch};

use md3_model::Account;
use mf_platform::{SettingKey, SettingsStore};

use yt_api::{YtClient, YtError};
use yt_auth::Store;
use yt_player::PlayerHandle;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AuthState {
    #[default]
    Idle,
    Waiting,
    Ready,
    Failed,
}
impl AuthState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Waiting => "waiting",
            Self::Ready => "ready",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlaybackAuthState {
    #[default]
    Unavailable,
    Authorizing,
    Authorized,
    Failed,
}
impl PlaybackAuthState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unavailable => "unavailable",
            Self::Authorizing => "authorizing",
            Self::Authorized => "authorized",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AuthSnapshot {
    pub state: AuthState,
    pub error_text: String,
    pub auth_url: String,
    pub auth_port: u16,
    pub playback: PlaybackAuthState,
    pub playback_error: String,
    pub error_code: String,
    pub playback_error_code: String,
    pub account: Option<Account>,
    pub token_storage: String,
    pub personal_client_id: String,
    pub personal_state: String,
    pub web_state: String,
    pub proxy_state: String,
    pub proxy_error: String,
    pub engine_restart_pending: bool,
    // YTMusic extras
    pub has_headers: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AuthEvent {
    SignedIn(Account),
    SignedOut,
    PlaybackReady(String),
    PremiumRequired,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProxyInput {
    pub mode: String,
    pub host: String,
    pub port: String,
    pub username: String,
    pub password: String,
}

pub struct AuthService {
    rt: Handle,
    settings: Arc<SettingsStore>,
    api: Arc<YtClient>,
    player: PlayerHandle,
    store: Store,
    snapshot: watch::Sender<AuthSnapshot>,
    events: broadcast::Sender<AuthEvent>,
}

impl AuthService {
    pub fn new(
        rt: Handle,
        settings: Arc<SettingsStore>,
        api: Arc<YtClient>,
        player: PlayerHandle,
        store: Store,
    ) -> Arc<Self> {
        let (snapshot, _) = watch::channel(AuthSnapshot { has_headers: store.has_auth(), web_state: "none".into(), personal_state: "none".into(), ..Default::default() });
        let (events, _) = broadcast::channel(64);
        Arc::new(Self { rt, settings, api, player, store, snapshot, events })
    }

    pub fn start(&self) {
        let api = self.api.clone();
        let snapshot = self.snapshot.clone();
        let events = self.events.clone();
        let has = self.store.has_auth();
        if !has {
            let _ = snapshot.send(AuthSnapshot { web_state: "none".into(), personal_state: "none".into(), ..Default::default() });
            return;
        }
        let rt = self.rt.clone();
        rt.spawn(async move {
            match api.get_library_playlists(1).await {
                Ok(_) => {
                    let account = Account { display_name: "YouTube Music".into(), product: "premium".into(), premium: true, ..Default::default() };
                    let snap = AuthSnapshot {
                        state: AuthState::Ready,
                        playback: PlaybackAuthState::Authorized,
                        account: Some(account.clone()),
                        web_state: "ready".into(),
                        personal_state: "ready".into(),
                        has_headers: true,
                        ..Default::default()
                    };
                    let _ = snapshot.send(snap);
                    let _ = events.send(AuthEvent::SignedIn(account));
                    let _ = events.send(AuthEvent::PlaybackReady("this-computer".into()));
                }
                Err(_) => {
                    let _ = snapshot.send(AuthSnapshot { web_state: "error".into(), personal_state: "error".into(), ..Default::default() });
                }
            }
        });
    }

    pub fn start_login(&self) {
        let _ = yt_auth::open_browser("https://github.com/sigma67/ytmusicapi#setup");
    }
    pub fn start_web_login(&self) { self.start_login(); }
    pub fn reopen_browser(&self) { self.start_login(); }
    pub fn cancel_login(&self) {}
    pub fn sign_out(&self) {
        self.store.clear();
        let _ = self.snapshot.send(AuthSnapshot { web_state: "none".into(), personal_state: "none".into(), ..Default::default() });
        let _ = self.events.send(AuthEvent::SignedOut);
    }
    pub fn retry(&self) { self.start(); }
    pub fn refresh_profile(&self) { self.start(); }
    pub fn authorize_playback(&self) { self.start(); }
    pub fn disable_playback(&self) {}
    pub fn restart_engine(&self) {}
    pub fn set_personal_client_id(&self, _client_id: Option<String>) {}
    pub fn apply_proxy(&self, input: ProxyInput) {
        let _ = self.settings.set(SettingKey::ProxyMode, mf_platform::SettingValue::Str(input.mode.clone()));
    }

    pub fn save_headers(&self, json: &str) -> Result<(), String> {
        self.store.save(json).map_err(|e| e.to_string()).map(|_| {
            self.start();
        })
    }

    pub fn is_signed_in(&self) -> bool {
        self.snapshot.borrow().account.is_some()
    }
    pub fn is_premium(&self) -> bool {
        self.snapshot.borrow().account.as_ref().is_some_and(|a| a.premium)
    }
    pub fn account(&self) -> Option<Account> {
        self.snapshot.borrow().account.clone()
    }
    pub fn account_id(&self) -> Option<md3_model::AccountId> {
        None
    }
    pub fn api(&self) -> Arc<YtClient> {
        self.api.clone()
    }
    pub fn snapshot(&self) -> AuthSnapshot {
        self.snapshot.borrow().clone()
    }
    pub fn watch(&self) -> watch::Receiver<AuthSnapshot> {
        self.snapshot.subscribe()
    }
    pub fn subscribe(&self) -> broadcast::Receiver<AuthEvent> {
        self.events.subscribe()
    }
    pub fn proxy(&self) -> crate::ProxyConfig {
        crate::ProxyConfig::System
    }
    pub fn watch_proxy(&self) -> watch::Receiver<crate::ProxyConfig> {
        let (tx, rx) = watch::channel(crate::ProxyConfig::System);
        let _ = tx;
        rx
    }
    pub fn note_api_error(&self, _error: &YtError) {}
    pub fn player(&self) -> &PlayerHandle {
        &self.player
    }
}

/// Compat shim: old sf-services exposed `Grant`.
pub struct Grant;
impl Grant {
    pub fn shared_web_api() -> Self { Self }
    pub fn redirect_uri(&self) -> String { "https://music.youtube.com".to_string() }
}
