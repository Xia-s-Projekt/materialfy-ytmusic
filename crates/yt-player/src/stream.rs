//! Pick the best audio-only format from `get_song`'s `streamingData`.
//!
//! YouTube offers AAC in `audio/mp4` and Opus in `audio/webm`. Symphonia (the
//! decoder behind rodio) has no Opus decoder, so an MP4 stream always wins;
//! bitrate only breaks ties inside the same container.

use serde_json::Value;

/// Whether rodio's symphonia backend can decode this MIME type.
fn decodable(mime: &str) -> bool {
    mime.starts_with("audio/mp4")
}

/// YouTube often returns only `signatureCipher` (no plain `url`): it's a
/// query string `s=<SIG>&sp=sig&url=<base>&...`. The playable URL is
/// `<base>&sig=<SIG>` (sp=sig convention). Decode percent-encoding for `url=`.
fn url_from_cipher(cipher: &str) -> Option<String> {
    let mut base = None;
    let mut sig = None;
    for pair in cipher.split('&') {
        let (k, v) = pair.split_once('=')?;
        if k == "url" && base.is_none() {
            base = Some(percent_decode(v));
        } else if k == "s" && sig.is_none() {
            sig = Some(percent_decode(v));
        }
    }
    match (base, sig) {
        (Some(b), Some(s)) if !b.is_empty() && !s.is_empty() => Some(format!("{b}&sig={s}")),
        _ => None,
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && bytes.get(i + 2).is_some_and(|_| bytes[i + 1].is_ascii_hexdigit()) {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("0");
            if let Ok(n) = u8::from_str_radix(hex, 16) {
                out.push(n);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn best_of<'a>(formats: impl Iterator<Item = &'a Value>) -> Option<(String, u32)> {
    let mut best: Option<(String, u32, bool)> = None;
    for fmt in formats {
        let mime = fmt.get("mimeType").and_then(Value::as_str).unwrap_or("");
        if !mime.starts_with("audio/") {
            continue;
        }
        let bitrate = fmt.get("bitrate").and_then(Value::as_u64).unwrap_or(0) as u32;
        let direct = fmt.get("url").and_then(Value::as_str).unwrap_or("");
        let url: String = if !direct.is_empty() {
            direct.to_string()
        } else if let Some(c) = fmt.get("signatureCipher").and_then(Value::as_str) {
            match url_from_cipher(c) {
                Some(u) => u,
                None => continue,
            }
        } else {
            continue;
        };
        let url = url.as_str();
        let usable = decodable(mime);
        let better = match &best {
            None => true,
            Some((_, b, was_usable)) => (usable, bitrate) > (*was_usable, *b),
        };
        if better {
            best = Some((url.to_string(), bitrate, usable));
        }
    }
    best.map(|(url, bitrate, _)| (url, bitrate))
}

/// Best audio-only URL, preferring a container rodio can decode, or None.
pub fn pick_audio_url(song: &Value) -> Option<(String, u32)> {
    let data = song.get("streamingData")?;
    let adaptive = data
        .get("adaptiveFormats")
        .and_then(Value::as_array)
        .map(|a| a.as_slice())
        .unwrap_or(&[]);
    if let Some(found) = best_of(adaptive.iter()) {
        return Some(found);
    }
    // fall back to formats[]
    best_of(
        data.get("formats")
            .and_then(Value::as_array)
            .map(|a| a.as_slice())
            .unwrap_or(&[])
            .iter(),
    )
}

/// Output device names (rodio defaults to system default; listed for Settings).
pub fn output_devices() -> Vec<String> {
    vec!["System Default".to_string()]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn picks_highest_audio_bitrate() {
        let song = serde_json::json!({"streamingData": {"adaptiveFormats": [
            {"mimeType":"video/webm","bitrate":999,"url":"v"},
            {"mimeType":"audio/webm; codecs=\"opus\"","bitrate":128000,"url":"lo"},
            {"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","bitrate":160000,"url":"hi"}
        ]}});
        assert_eq!(pick_audio_url(&song).unwrap().0, "hi");
        assert_eq!(pick_audio_url(&serde_json::json!({})), None);
    }

    #[test]
    fn prefers_decodable_mp4_over_higher_bitrate_opus() {
        let song = serde_json::json!({"streamingData": {"adaptiveFormats": [
            {"mimeType":"audio/webm; codecs=\"opus\"","bitrate":160000,"url":"opus"},
            {"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","bitrate":128000,"url":"aac"},
            {"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","bitrate":64000,"url":"aac-lo"}
        ]}});
        assert_eq!(pick_audio_url(&song).unwrap(), ("aac".to_string(), 128000));
    }

    #[test]
    fn decodes_signature_cipher_formats() {
        // Real shape from get_song: only signatureCipher, no url.
        let cipher = "s=SIG123&sp=sig&url=https%3A%2F%2Frr1.googlevideo.com%2Fvideoplayback%3Fitag%3D140%26x%3D1&itag=140";
        let song = serde_json::json!({"streamingData": {"adaptiveFormats": [
            {"mimeType":"audio/mp4; codecs=\"mp4a.40.2\"","bitrate":128000,"signatureCipher": cipher}
        ]}});
        let (url, _) = pick_audio_url(&song).expect("cipher format should yield a URL");
        assert!(url.starts_with("https://rr1.googlevideo.com/videoplayback?itag=140"), "got: {url}");
        assert!(url.ends_with("&sig=SIG123"), "got: {url}");
    }

    #[test]
    fn falls_back_to_opus_when_no_mp4() {
        let song = serde_json::json!({"streamingData": {"adaptiveFormats": [
            {"mimeType":"audio/webm; codecs=\"opus\"","bitrate":160000,"url":"opus"}
        ]}});
        assert_eq!(pick_audio_url(&song).unwrap().0, "opus");
    }
}
