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

    #[test]
    fn cookie_export_array_normalizes_to_cookie_header() {
        let pasted = r#"[
            {"name":"VISITOR_PRIVACY_METADATA","value":"Cg==","domain":".youtube.com","hostOnly":false,"path":"/","secure":true,"httpOnly":true,"session":false,"expirationDate":1809764109.145},
            {"name":"__Secure-3PAPISID","value":"abc123","domain":".youtube.com","hostOnly":false,"path":"/","secure":true,"httpOnly":false,"session":false,"expirationDate":1828717225.789},
            {"name":"SID","value":"sid-val","domain":".youtube.com","hostOnly":false,"path":"/","secure":false,"httpOnly":false,"session":false,"expirationDate":1828717225.789}
        ]"#;
        let normalized = HeadersAuth::normalize(pasted).expect("cookie export should normalize");
        let v: serde_json::Value = serde_json::from_str(&normalized).unwrap();
        let cookie = v.get("cookie").and_then(|c| c.as_str()).unwrap();
        assert!(cookie.contains("__Secure-3PAPISID=abc123"), "got: {cookie}");
        assert!(cookie.contains("SID=sid-val"), "got: {cookie}");
        assert!(cookie.contains("VISITOR_PRIVACY_METADATA=Cg=="), "got: {cookie}");
    }

    #[test]
    fn cookie_export_without_papisid_is_rejected() {
        let pasted = r#"[{"name":"SID","value":"x","domain":".youtube.com"}]"#;
        assert!(HeadersAuth::normalize(pasted).is_err());
    }

    #[test]
    fn raw_cookie_string_normalizes_to_cookie_header() {
        let pasted = "VISITOR_PRIVACY_METADATA=Cg==; __Secure-3PAPISID=abc123; SID=sid-val";
        let normalized = HeadersAuth::normalize(pasted).expect("cookie string should normalize");
        let v: serde_json::Value = serde_json::from_str(&normalized).unwrap();
        let cookie = v.get("cookie").and_then(|c| c.as_str()).unwrap();
        assert!(cookie.contains("__Secure-3PAPISID=abc123"), "got: {cookie}");
        assert!(cookie.contains("SID=sid-val"), "got: {cookie}");
    }

    #[test]
    fn raw_cookie_string_with_prefix_normalizes() {
        let pasted = "Cookie: SID=sid-val; __Secure-3PAPISID=abc123";
        let normalized = HeadersAuth::normalize(pasted).expect("prefixed cookie should normalize");
        let v: serde_json::Value = serde_json::from_str(&normalized).unwrap();
        assert!(v.get("cookie").and_then(|c| c.as_str()).unwrap_or("").contains("__Secure-3PAPISID=abc123"));
    }

    #[test]
    fn raw_cookie_string_without_papisid_is_rejected() {
        let pasted = "SID=x; YSC=y";
        assert!(HeadersAuth::normalize(pasted).is_err());
    }
}
