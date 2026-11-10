//! Downloading the current track's cover to a file for the theme (mf-theme) and SMTC/MPRIS:
//! <cover_dir>/<sha1(url)>.jpg; an already downloaded file is not downloaded again. The write is atomic (`.part` → rename).
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use sha1::{Digest, Sha1};

/// A cover larger than this is refused (as in spotifast).
const MAX_COVER_BYTES: usize = 8 * 1024 * 1024;

/// File path for a URL (without downloading).
pub fn cover_path(cover_dir: &Path, url: &str) -> PathBuf {
    cover_dir.join(format!("{}.jpg", hex::encode(Sha1::digest(url.as_bytes()))))
}

/// Download (through the shared HTTP client with proxy) and return the path; an error is text.
pub async fn fetch_cover(http: &sf_api::Http, cover_dir: &Path, url: &str) -> Result<PathBuf, String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(format!("cover: unsupported address {url:?}"));
    }
    let path = cover_path(cover_dir, url);
    if tokio::fs::metadata(&path).await.is_ok_and(|meta| meta.is_file() && meta.len() > 0) {
        return Ok(path);
    }
    // Spotify's picture — in turn from each of its CDNs (i.scdn.co does not open on some networks)
    let client = http.client()?;
    let mut failure = String::from("cover: no address");
    let mut fetched = None;
    for address in sf_api::image_hosts::mirrors(url) {
        let attempt = async {
            let response = client
                .get(&address)
                .timeout(std::time::Duration::from_secs(10))
                .send()
                .await
                .map_err(|error| format!("cover: {}", error.without_url()))?;
            if !response.status().is_success() {
                return Err(format!("cover: response {}", response.status()));
            }
            response
                .bytes()
                .await
                .map_err(|error| format!("cover: {}", error.without_url()))
        };
        match attempt.await {
            Ok(bytes) => {
                fetched = Some(bytes);
                break;
            }
            Err(error) => failure = error,
        }
    }
    let bytes = fetched.ok_or(failure)?;
    if bytes.is_empty() {
        return Err("cover: empty response".into());
    }
    if bytes.len() > MAX_COVER_BYTES {
        return Err("cover: file too large".into());
    }
    tokio::fs::create_dir_all(cover_dir)
        .await
        .map_err(|error| format!("cover: {error}"))?;
    // A temporary file of its own for each download: two simultaneous requests for one URL do not interfere.
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let temporary = path.with_extension(format!(
        "{}-{}.part",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    if let Err(error) = tokio::fs::write(&temporary, &bytes).await {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(format!("cover: {error}"));
    }
    if let Err(error) = tokio::fs::rename(&temporary, &path).await {
        let _ = tokio::fs::remove_file(&temporary).await;
        // Another downloader managed to put the same file — that is a success.
        if tokio::fs::metadata(&path).await.is_ok_and(|meta| meta.len() > 0) {
            return Ok(path);
        }
        return Err(format!("cover: {error}"));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    async fn serve(status: &'static str, bytes: Vec<u8>, times: usize) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/cover", listener.local_addr().unwrap());
        tokio::spawn(async move {
            for _ in 0..times {
                let Ok(Ok((mut socket, _))) =
                    tokio::time::timeout(Duration::from_secs(10), listener.accept()).await
                else {
                    return;
                };
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    request.push(socket.read_u8().await.unwrap());
                }
                let header = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    bytes.len()
                );
                socket.write_all(header.as_bytes()).await.unwrap();
                socket.write_all(&bytes).await.unwrap();
            }
        });
        url
    }

    fn http() -> sf_api::Http {
        sf_api::Http::new(reqwest::Client::builder().no_proxy().build().unwrap())
    }

    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("sf-player-cover-{name}-{}", std::process::id()))
    }

    #[test]
    fn the_path_is_the_url_digest_with_jpg() {
        let path = cover_path(Path::new("covers"), "https://i.scdn.co/image/ab67");
        let name = path.file_name().unwrap().to_str().unwrap();
        assert_eq!(name.len(), 40 + 4);
        assert!(name.ends_with(".jpg"));
        assert_eq!(path, cover_path(Path::new("covers"), "https://i.scdn.co/image/ab67"));
        assert_ne!(path, cover_path(Path::new("covers"), "https://i.scdn.co/image/ab68"));
    }

    #[tokio::test]
    async fn a_downloaded_cover_is_kept_and_not_fetched_again() {
        let dir = scratch("roundtrip");
        let _ = std::fs::remove_dir_all(&dir);
        let expected: Vec<u8> = (0..64 * 1024).map(|i| (i % 251) as u8).collect();
        // The server answers once: the second call must take the file.
        let url = serve("200 OK", expected.clone(), 1).await;
        let first = fetch_cover(&http(), &dir, &url).await.unwrap();
        assert_eq!(std::fs::read(&first).unwrap(), expected);
        let second = fetch_cover(&http(), &dir, &url).await.unwrap();
        assert_eq!(first, second);
        let leftovers = std::fs::read_dir(&dir)
            .unwrap()
            .filter(|entry| entry.as_ref().unwrap().path().to_string_lossy().ends_with(".part"))
            .count();
        assert_eq!(leftovers, 0);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn a_refused_cover_leaves_no_file() {
        let dir = scratch("refused");
        let _ = std::fs::remove_dir_all(&dir);
        let url = serve("404 Not Found", b"nope".to_vec(), 1).await;
        assert!(fetch_cover(&http(), &dir, &url).await.is_err());
        assert!(!cover_path(&dir, &url).exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_non_http_address_is_refused() {
        assert!(fetch_cover(&http(), Path::new("."), "file:///etc/passwd").await.is_err());
    }
}
