//! InnerTube player client: resolve Google Video audio URLs directly via
//! `youtubei/v1/player` instead of shelling to yt-dlp.
//!
//! The anonymous `ANDROID` client returns plain `url` formats (no
//! `signatureCipher`), so the audio thread can stream bytes straight from the
//! returned CDN URL with bounded HTTP `Range` requests. This is the same
//! request ytmusicapi's `get_song` cannot make for us: its signed-in
//! `WEB_REMIX` context only returns ciphered URLs.

use serde_json::{Value, json};

use crate::client::YtError;

const PLAYER_URL: &str = "https://www.youtube.com/youtubei/v1/player";
const ANDROID_KEY: &str = "AIzaSyAO_FJ2SlqU8Q4STEHLGCilw_Y9_11qcW8";
const ANDROID_VERSION: &str = "20.10.38";
const ANDROID_UA: &str =
    "com.google.android.youtube/20.10.38 (Linux; U; Android 14; en_US) gzip";

fn android_client() -> Value {
    json!({
        "clientName": "ANDROID",
        "clientVersion": ANDROID_VERSION,
        "androidSdkVersion": 34,
        "osVersion": "14",
        "deviceMake": "Google",
        "deviceModel": "Pixel 9",
        "hl": "en",
        "gl": "US",
    })
}

/// POST to InnerTube player and return the raw JSON response.
async fn player(video_id: &str) -> Result<Value, YtError> {
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| YtError::Network(e.to_string()))?;
    let body = json!({
        "context": { "client": android_client() },
        "videoId": video_id,
        "playbackContext": {
            "contentPlaybackContext": { "signatureTimestamp": "20000" }
        },
        "contentCheckOk": true,
        "racyCheckOk": true,
    });
    let res = http
        .post(format!("{PLAYER_URL}?key={ANDROID_KEY}"))
        .header("User-Agent", ANDROID_UA)
        .header("X-YouTube-Client-Name", "3")
        .header("X-YouTube-Client-Version", ANDROID_VERSION)
        .json(&body)
        .send()
        .await
        .map_err(|e| YtError::Network(e.to_string()))?;
    if !res.status().is_success() {
        return Err(YtError::Remote(format!("InnerTube player HTTP {}", res.status())));
    }
    res.json::<Value>()
        .await
        .map_err(|e| YtError::Remote(e.to_string()))
}

/// Rank an audio MIME type by how likely rodio/symphonia is to decode it.
/// Symphonia has no Opus decoder, and HE-AAC (SBR) is less reliable than
/// AAC-LC, so plain `mp4a.40.2` wins.
fn rank(mime: &str) -> u8 {
    if mime.contains("mp4a.40.2") {
        4
    } else if mime.starts_with("audio/mp4") {
        3
    } else if mime.contains("opus") || mime.contains("webm") {
        2
    } else {
        1
    }
}

/// Pick an audio-only format. Returns `(url, bitrate, mime)`.
pub fn pick_audio(data: &Value) -> Option<(String, u32, String)> {
    let formats = data
        .get("streamingData")?
        .get("adaptiveFormats")?
        .as_array()?;
    let mut best: Option<(String, u32, String, u8)> = None;
    for fmt in formats {
        let mime = fmt.get("mimeType").and_then(Value::as_str).unwrap_or("");
        if !mime.starts_with("audio/") {
            continue;
        }
        let Some(url) = fmt.get("url").and_then(Value::as_str).filter(|u| !u.is_empty()) else {
            continue;
        };
        let bitrate = fmt
            .get("bitrate")
            .or_else(|| fmt.get("audioBitrate"))
            .and_then(Value::as_u64)
            .unwrap_or(0) as u32;
        let rank = rank(mime);
        let better = match &best {
            None => true,
            Some((_, best_bitrate, _, best_rank)) => {
                rank > *best_rank || (rank == *best_rank && bitrate > *best_bitrate)
            }
        };
        if better {
            best = Some((url.to_string(), bitrate, mime.to_string(), rank));
        }
    }
    best.map(|(url, bitrate, mime, _)| (url, bitrate, mime))
}

/// Resolve an audio-only stream URL via InnerTube.
/// Returns `{"url": ..., "bitrate": ..., "mime": ..., "title": ..., "artist": ..., "cover": ..., "durationMs": ...}`.
pub async fn stream_url(video_id: &str) -> Result<Value, YtError> {
    let data = player(video_id).await?;
    if let Some((url, bitrate, mime)) = pick_audio(&data) {
        let mut out = json!({ "url": url, "bitrate": bitrate, "mime": mime });
        if let Some(meta) = video_details(&data) {
            if let Some(map) = out.as_object_mut() {
                for (key, value) in meta {
                    map.insert(key.to_string(), value);
                }
            }
        }
        return Ok(out);
    }
    let status = data
        .get("playabilityStatus")
        .and_then(|p| p.get("status"))
        .and_then(Value::as_str)
        .unwrap_or("UNKNOWN");
    if status != "OK" {
        let reason = data
            .get("playabilityStatus")
            .and_then(|p| p.get("reason"))
            .and_then(Value::as_str)
            .unwrap_or(status);
        return Err(YtError::Remote(format!("InnerTube: {reason}")));
    }
    Err(YtError::Remote("No playable audio from InnerTube".into()))
}

/// Metadata only (no stream URL): `{"title": ..., "artist": ..., "cover": ..., "durationMs": ...}`.
/// Used to enrich queue rows that were created from bare URIs.
pub async fn video_details_json(video_id: &str) -> Result<Value, YtError> {
    let data = player(video_id).await?;
    let meta = video_details(&data).unwrap_or_default();
    let mut out = serde_json::Map::new();
    for (key, value) in meta {
        out.insert(key.to_string(), value);
    }
    Ok(Value::Object(out))
}

/// Track metadata from the player's `videoDetails`: title, channel (artist), artwork, length.
/// The channel of an auto-generated "Topic" track is the artist, so the "- Topic" suffix is dropped.
fn video_details(data: &Value) -> Option<Vec<(&'static str, Value)>> {
    let details = data.get("videoDetails")?;
    let title = details.get("title").and_then(Value::as_str).filter(|t| !t.is_empty())?;
    let author = details.get("author").and_then(Value::as_str).unwrap_or("");
    let artist = author.strip_suffix(" - Topic").unwrap_or(author);
    let duration_ms = details
        .get("lengthSeconds")
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<i64>().ok())
        .map(|s| s * 1000)
        .unwrap_or(0);
    let cover = details
        .get("thumbnail")
        .and_then(|t| t.get("thumbnails"))
        .and_then(Value::as_array)
        .map(|thumbs| crate::util::pick_thumbnail(thumbs, crate::map::COVER_LARGE_PX))
        .unwrap_or_default();
    let mut out = vec![("title", Value::from(title))];
    if !artist.is_empty() {
        out.push(("artist", Value::from(artist)));
    }
    if duration_ms > 0 {
        out.push(("durationMs", Value::from(duration_ms)));
    }
    if !cover.is_empty() {
        out.push(("cover", Value::from(cover)));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_aac_lc_over_opus_and_he_aac() {
        let data = json!({
            "streamingData": {
                "adaptiveFormats": [
                    { "mimeType": "audio/webm; codecs=\"opus\"", "url": "https://x/opus", "bitrate": 192000 },
                    { "mimeType": "audio/mp4; codecs=\"mp4a.40.5\"", "url": "https://x/he", "bitrate": 256000 },
                    { "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"", "url": "https://x/aac", "bitrate": 128000 },
                    { "mimeType": "video/mp4", "url": "https://x/vid", "bitrate": 0 }
                ]
            }
        });
        let (url, br, mime) = pick_audio(&data).expect("picks");
        assert_eq!(url, "https://x/aac");
        assert_eq!(br, 128000);
        assert!(mime.contains("mp4a.40.2"));
    }

    #[test]
    fn ignores_ciphered_formats() {
        let data = json!({
            "streamingData": { "adaptiveFormats": [
                { "mimeType": "audio/mp4", "signatureCipher": "s=abc", "bitrate": 128000 }
            ]}
        });
        assert!(pick_audio(&data).is_none());
    }

    #[test]
    fn empty_when_no_audio() {
        let data = json!({ "streamingData": { "adaptiveFormats": [] } });
        assert!(pick_audio(&data).is_none());
    }
}
