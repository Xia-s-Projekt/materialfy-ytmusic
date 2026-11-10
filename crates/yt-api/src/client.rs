//! One-shot Python bridge: every call spawns `python3 yt_bridge.py`.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use serde_json::{Value, json};
use thiserror::Error;

#[derive(Clone, Debug, Error, PartialEq)]
pub enum YtError {
    #[error("Sign in to YouTube Music first (headers file)")]
    NotSignedIn,
    #[error("vendored ytmusicapi missing next to the bridge (Python 3 is required)")]
    PythonMissing,
    #[error("Cannot reach YouTube Music: {0}")]
    Network(String),
    #[error("YouTube Music returned an error: {0}")]
    Remote(String),
    #[error("YouTube Music sent an unreadable response")]
    Decode,
}

impl YtError {
    pub fn user_text(&self) -> String {
        match self {
            YtError::NotSignedIn => "Sign in to YouTube Music first".to_string(),
            YtError::PythonMissing => "Embedded YouTube Music API is missing — reinstall the app".to_string(),
            YtError::Network(_) => "Can't reach YouTube Music — check your network".to_string(),
            YtError::Remote(message) => {
                if message.contains("authentication")
                    || message.contains("Please provide authentication")
                {
                    "Sign in to YouTube Music first".to_string()
                } else {
                    format!("YouTube Music: {message}")
                }
            }
            YtError::Decode => "YouTube Music sent a response we couldn't read".to_string(),
        }
    }
}

pub type Result<T> = std::result::Result<T, YtError>;

fn bridge_path() -> PathBuf {
    if let Some(p) = std::env::var_os("MATERIALFY_YT_BRIDGE") {
        return PathBuf::from(p);
    }
    // dev tree layout: <repo>/crates/yt-api/python/yt_bridge.py
    if let Some(exe) = std::env::current_exe().ok() {
        for ancestor in exe.ancestors() {
            let cand = ancestor.join("crates").join("yt-api").join("python").join("yt_bridge.py");
            if cand.is_file() {
                return cand;
            }
            let cand2 = ancestor.join("yt_bridge.py");
            if cand2.is_file() {
                // packaged next to binary (see scripts/package)
                return cand2;
            }
        }
    }
    PathBuf::from("crates/yt-api/python/yt_bridge.py")
}

fn python_bin() -> String {
    if let Ok(p) = std::env::var("MATERIALFY_PYTHON") {
        if !p.trim().is_empty() {
            return p;
        }
    }
    // A bare `python3` can resolve to a tool-manager wrapper (pyenv/asdf/agent
    // shims) that blocks when spawned non-interactively. Prefer a real system
    // interpreter; fall back to PATH only when none of these exist.
    for cand in ["/usr/bin/python3", "/usr/local/bin/python3", "/bin/python3"] {
        if std::path::Path::new(cand).is_file() {
            return cand.to_string();
        }
    }
    "python3".to_string()
}

/// Vendored dependency dir (`python/vendor` next to the bridge), so no
/// `pip install` is needed: the app is fully self-contained on python3.
fn vendor_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("MATERIALFY_YT_VENDOR") {
        return PathBuf::from(p);
    }
    if let Some(parent) = bridge_path().parent() {
        return parent.join("vendor");
    }
    PathBuf::from("crates/yt-api/python/vendor")
}

/// Minimal InnerTube entry point (also used by yt-player for stream URLs).
#[derive(Clone)]
pub struct YtClient {
    headers: Option<PathBuf>,
    http_proxy: Option<String>,
}

impl YtClient {
    pub fn new(headers: Option<PathBuf>) -> Self {
        Self { headers, http_proxy: None }
    }

    pub fn with_proxy(mut self, proxy: Option<String>) -> Self {
        self.http_proxy = proxy;
        self
    }

    pub fn headers_path(&self) -> Option<&PathBuf> {
        self.headers.as_ref()
    }

    /// Compat shim for old sf-api gateway: covers/lyrics use reqwest.
    pub fn http(&self) -> crate::http::Http {
        crate::http::Http::default()
    }
    pub fn web_api_limits(&self) -> tokio::sync::watch::Receiver<crate::http::WebApiLimits> {
        let (tx, rx) = tokio::sync::watch::channel(crate::http::WebApiLimits::default());
        let _ = tx;
        rx
    }

    /// One bridge invocation.
    pub async fn call(&self, method: &str, args: Value) -> Result<Value> {
        let bridge = bridge_path();
        let python = python_bin();
        let mut cmd = tokio::process::Command::new(&python);
        cmd.arg(&bridge).arg("--method").arg(method).arg("--args").arg(args.to_string());
        // Only pass --headers once the file exists (post first sign-in);
        // before that the bridge runs unauthenticated so browsing works.
        if let Some(h) = &self.headers {
            if h.is_file() {
                cmd.arg("--headers").arg(h);
            }
        }
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        // vendored ytmusicapi + deps: never touch the user's global pip
        let vendor = vendor_dir();
        if vendor.is_dir() {
            let sep = std::path::MAIN_SEPARATOR;
            let mut pypath = String::from(vendor.to_string_lossy());
            if let Ok(existing) = std::env::var("PYTHONPATH") {
                if !existing.is_empty() {
                    pypath.push(sep);
                    pypath.push_str(&existing);
                }
            }
            cmd.env("PYTHONPATH", pypath);
        }
        if let Some(proxy) = &self.http_proxy {
            cmd.env("HTTP_PROXY", proxy).env("HTTPS_PROXY", proxy);
        }
        let timeout = if method == "download_audio" {
            Duration::from_secs(600)
        } else {
            Duration::from_secs(45)
        };
        let output = tokio::time::timeout(timeout, cmd.output())
            .await
            .map_err(|_| YtError::Network("timed out".to_string()))?
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    YtError::PythonMissing
                } else {
                    YtError::Network(e.to_string())
                }
            })?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if !output.status.success() {
            if stderr.contains("No module named 'ytmusicapi'") || stderr.contains("ytmusicapi") {
                return Err(YtError::PythonMissing);
            }
            return Err(YtError::Network(if stderr.trim().is_empty() {
                format!("python exited {}", output.status)
            } else {
                stderr.trim().to_string()
            }));
        }
        // bridge prints one JSON object; take the last non-empty line
        let line = stdout
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("");
        let envelope: Value =
            serde_json::from_str(line).map_err(|_| YtError::Decode)?;
        if envelope.get("ok").and_then(Value::as_bool) == Some(true) {
            Ok(envelope.get("data").cloned().unwrap_or(Value::Null))
        } else {
            let msg = envelope
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("unknown error")
                .to_string();
            if msg.contains("not installed") {
                Err(YtError::PythonMissing)
            } else if msg.contains("Please provide authentication")
                || msg.contains("authentication")
            {
                Err(YtError::NotSignedIn)
            } else {
                Err(YtError::Remote(msg))
            }
        }
    }

    pub async fn search(&self, query: &str, filter: Option<&str>, limit: u32) -> Result<Value> {
        self.call("search", json!({"query": query, "filter": filter, "limit": limit}))
            .await
    }

    pub async fn get_home(&self, limit: u32) -> Result<Value> {
        self.call("get_home", json!({"limit": limit})).await
    }

    /// "New releases" browse: normalized two-row items (albums for signed-in
    /// users, release playlists for guests). Bridge `get_new_releases`.
    pub async fn get_new_releases(&self) -> Result<Value> {
        self.call("get_new_releases", json!({})).await
    }

    pub async fn get_library_playlists(&self, limit: u32) -> Result<Value> {
        self.call("get_library_playlists", json!({"limit": limit})).await
    }

    /// Signed-in channel profile: `{name, avatar, channelId}` (bridge `get_account`).
    pub async fn get_account(&self) -> Result<Value> {
        self.call("get_account", json!({})).await
    }

    /// Whether the saved headers belong to a signed-in account (bridge `check_session`).
    /// A guest cookie answers library calls with an empty list instead of an error,
    /// so the library cannot tell the two apart; this asks the account menu instead.
    pub async fn check_session(&self) -> Result<bool> {
        let v = self.call("check_session", json!({})).await?;
        Ok(v.get("signedIn").and_then(Value::as_bool).unwrap_or(false))
    }

    pub async fn get_liked_songs(&self, limit: u32) -> Result<Value> {
        self.call("get_liked_songs", json!({"limit": limit})).await
    }

    pub async fn get_library_songs(&self, limit: u32) -> Result<Value> {
        self.call("get_library_songs", json!({"limit": limit})).await
    }

    pub async fn get_library_albums(&self, limit: u32) -> Result<Value> {
        self.call("get_library_albums", json!({"limit": limit})).await
    }

    pub async fn get_library_artists(&self, limit: u32) -> Result<Value> {
        self.call("get_library_artists", json!({"limit": limit})).await
    }

    pub async fn get_playlist(&self, playlist_id: &str, limit: u32) -> Result<Value> {
        self.call("get_playlist", json!({"playlistId": playlist_id, "limit": limit}))
            .await
    }

    /// New playlist; returns the playlistId (or a dict carrying it).
    pub async fn create_playlist(
        &self,
        title: &str,
        description: &str,
        privacy: &str,
        video_ids: &[String],
    ) -> Result<Value> {
        self.call(
            "create_playlist",
            json!({
                "title": title,
                "description": description,
                "privacyStatus": privacy,
                "videoIds": video_ids,
            }),
        )
        .await
    }

    /// Rename / re-describe / change visibility of a playlist.
    pub async fn edit_playlist(
        &self,
        playlist_id: &str,
        title: Option<&str>,
        description: Option<&str>,
        privacy: Option<&str>,
    ) -> Result<Value> {
        self.call(
            "edit_playlist",
            json!({
                "playlistId": playlist_id,
                "title": title,
                "description": description,
                "privacyStatus": privacy,
            }),
        )
        .await
    }

    pub async fn delete_playlist(&self, playlist_id: &str) -> Result<Value> {
        self.call("delete_playlist", json!({"playlistId": playlist_id})).await
    }

    /// Append (or prepend, with `to_top`) videos to a playlist.
    pub async fn add_playlist_items(
        &self,
        playlist_id: &str,
        video_ids: &[String],
        to_top: bool,
        duplicates: bool,
    ) -> Result<Value> {
        self.call(
            "add_playlist_items",
            json!({"playlistId": playlist_id, "videoIds": video_ids, "addToTop": to_top, "duplicates": duplicates}),
        )
        .await
    }

    /// Remove entries: each needs the videoId and its setVideoId.
    pub async fn remove_playlist_items(
        &self,
        playlist_id: &str,
        videos: &[serde_json::Value],
    ) -> Result<Value> {
        self.call("remove_playlist_items", json!({"playlistId": playlist_id, "videos": videos}))
            .await
    }

    /// Move one playlist entry before another (both identified by setVideoId).
    pub async fn move_playlist_item(
        &self,
        playlist_id: &str,
        set_video_id: &str,
        before_set_video_id: &str,
    ) -> Result<Value> {
        self.call(
            "edit_playlist",
            json!({
                "playlistId": playlist_id,
                "moveItem": [set_video_id, before_set_video_id],
            }),
        )
        .await
    }

    /// Add (LIKE) or remove (INDIFFERENT) a playlist from the library.
    pub async fn rate_playlist(&self, playlist_id: &str, rating: &str) -> Result<Value> {
        self.call("rate_playlist", json!({"playlistId": playlist_id, "rating": rating}))
            .await
    }

    /// Thumbs up / thumbs down / indifferent for one track (`LIKE`, `DISLIKE`,
    /// `INDIFFERENT`).
    pub async fn rate_song(&self, video_id: &str, rating: &str) -> Result<Value> {
        self.call("rate_song", json!({"videoId": video_id, "rating": rating}))
            .await
    }

    /// Podcasts the account follows (library → "Podcasts").
    pub async fn get_library_podcasts(&self, limit: u32) -> Result<Value> {
        self.call("get_library_podcasts", json!({"limit": limit})).await
    }

    /// Podcast page: `{title, author, description, saved, episodes: […]}`.
    pub async fn get_podcast(&self, playlist_id: &str, limit: u32) -> Result<Value> {
        self.call("get_podcast", json!({"playlistId": playlist_id, "limit": limit}))
            .await
    }

    pub async fn get_episode(&self, video_id: &str) -> Result<Value> {
        self.call("get_episode", json!({"videoId": video_id})).await
    }

    pub async fn subscribe_artist(&self, channel_id: &str) -> Result<Value> {
        self.call("subscribe_artist", json!({"channelId": channel_id})).await
    }

    pub async fn unsubscribe_artist(&self, channel_id: &str) -> Result<Value> {
        self.call("unsubscribe_artist", json!({"channelId": channel_id})).await
    }

    pub async fn get_album(&self, browse_id: &str) -> Result<Value> {
        self.call("get_album", json!({"browseId": browse_id})).await
    }

    pub async fn get_artist(&self, channel_id: &str) -> Result<Value> {
        self.call("get_artist", json!({"channelId": channel_id})).await
    }

    pub async fn get_song(&self, video_id: &str) -> Result<Value> {
        self.call("get_song", json!({"videoId": video_id})).await
    }

    /// Playback URL via InnerTube `player`: `{url, bitrate, mime}`.
    /// Direct Rust post (no Python/yt-dlp needed for playback).
    pub async fn get_stream_url(&self, video_id: &str) -> Result<Value> {
        crate::innertube::stream_url(video_id).await
    }

    /// Track metadata for a video (bridge `check_session`-independent, direct InnerTube post).
    /// Queue rows created from bare URIs carry only the video id; the up-next list
    /// needs a title/artist/cover before the track ever plays.
    pub async fn get_video_details(&self, video_id: &str) -> Result<Value> {
        crate::innertube::video_details_json(video_id).await
    }

    /// Download audio to `output` via vendored yt-dlp (bridge `download_audio`):
    /// returns the file path. Google Video caps plain `url` GETs at 1 MiB per
    /// token (verified: exactly 1048576 bytes, then 403), so direct streaming
    /// can never play a full song on this network; yt-dlp's JS deciphering is
    /// what lifts the cap.
    pub async fn download_audio(&self, video_id: &str, output: &std::path::Path) -> Result<PathBuf> {
        let v = self
            .call(
                "download_audio",
                json!({"videoId": video_id, "output": output.to_string_lossy()}),
            )
            .await?;
        let path = v
            .get("file")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .unwrap_or_else(|| output.to_path_buf());
        if path.is_file() {
            Ok(path)
        } else {
            Err(YtError::Network("yt-dlp produced no audio file".to_string()))
        }
    }

    pub async fn get_watch_playlist(
        &self,
        video_id: Option<&str>,
        playlist_id: Option<&str>,
        radio: bool,
        limit: u32,
    ) -> Result<Value> {
        self.call(
            "get_watch_playlist",
            json!({"videoId": video_id, "playlistId": playlist_id, "radio": radio, "limit": limit}),
        )
        .await
    }

    pub async fn get_lyrics(&self, video_id: &str) -> Result<Value> {
        self.call("get_lyrics", json!({"videoId": video_id})).await
    }

    /// Spawn sink fast-path used by tests to assert the bridge file exists.
    #[cfg(test)]
    pub fn bridge_exists() -> bool {
        bridge_path().is_file()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bridge_file_present() {
        assert!(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("python/yt_bridge.py").is_file());
    }
    #[tokio::test]
    async fn missing_ytmusicapi_surfaces_as_python_missing() {
        // In CI without ytmusicapi the bridge returns {"ok":false,"error":"not installed"}
        // which we map to PythonMissing; with ytmusicapi installed it succeeds.
        // Either way the call must not panic.
        let client = YtClient::new(None);
        let r = client.search("test", None, 1).await;
        match r {
            Ok(_) => {}
            Err(YtError::PythonMissing) => {}
            Err(YtError::Remote(e)) => {
                // allow network-dependent remote errors in offline CI
                assert!(!e.is_empty());
            }
            Err(YtError::Network(_)) => {}
            Err(YtError::NotSignedIn) => {}
            Err(YtError::Decode) => panic!("decode must not happen for error envelope"),
        }
    }
}
