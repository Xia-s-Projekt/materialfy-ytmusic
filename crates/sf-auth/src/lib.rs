//! Signing in to Spotify without a UI: PKCE grants (oauth), secret storage (credentials), proxy (proxy).
//! The bottom sf-* crate: sf-api, sf-player and sf-services depend on it.

pub mod credentials;
pub mod oauth;
pub mod proxy;

pub use credentials::{Lease, Loaded, ProxyPassword, Slot, Store};
pub use oauth::{
    DEFAULT_WEB_CLIENT_ID, Flow, Grant, PLAYBACK_CLIENT_ID, PLAYBACK_REDIRECT_PORT, StoredToken, TokenEndpointError,
    TokenResponse, WEB_REDIRECT_PORT,
};
pub use proxy::{ManualProxy, ProxyConfig, ProxyMode};

/// Open a URL in the default browser (does not block).
pub fn open_browser(url: &str) -> Result<(), String> {
    open::that_detached(url).map_err(|e| e.to_string())
}
