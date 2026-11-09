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

fn best_of<'a>(formats: impl Iterator<Item = &'a Value>) -> Option<(String, u32)> {
    let mut best: Option<(String, u32, bool)> = None;
    for fmt in formats {
        let mime = fmt.get("mimeType").and_then(Value::as_str).unwrap_or("");
        if !mime.starts_with("audio/") {
            continue;
        }
        let bitrate = fmt.get("bitrate").and_then(Value::as_u64).unwrap_or(0) as u32;
        let url = fmt.get("url").and_then(Value::as_str).unwrap_or("");
        if url.is_empty() {
            continue;
        }
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
    fn falls_back_to_opus_when_no_mp4() {
        let song = serde_json::json!({"streamingData": {"adaptiveFormats": [
            {"mimeType":"audio/webm; codecs=\"opus\"","bitrate":160000,"url":"opus"}
        ]}});
        assert_eq!(pick_audio_url(&song).unwrap().0, "opus");
    }
}
