//! YouTube Music auth without UI: ytmusicapi `headers_auth` JSON stored in a private file
//! (OS keyring slot `materialfy-yt` where available, file fallback under the app state dir).
//!
//! ytmusicapi (sigma67) authenticates by replaying browser request headers
//! for music.youtube.com (`ytmusicapi --headers-auth`). The JSON is opaque:
//! we validate it parses and holds a `cookie` / `authorization` header, then hand
//! the file path to `yt-api`'s Python bridge per call.

pub mod store;

pub use store::{HeadersAuth, Store};

/// Open a URL in the default browser (does not block).
pub fn open_browser(url: &str) -> Result<(), String> {
    open::that_detached(url).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn headers_auth_validates_cookie_or_authorization() {
        assert!(HeadersAuth { json: r#"{"cookie":"a=1"}"#.into() }.is_valid());
        assert!(HeadersAuth {
            json: r#"{"authorization":"SAPISIDHASH x"}"#.into()
        }
        .is_valid());
        assert!(!HeadersAuth { json: r#"{}"#.into() }.is_valid());
        assert!(!HeadersAuth { json: "not json".into() }.is_valid());
    }
}
