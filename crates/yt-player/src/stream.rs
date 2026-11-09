//! Pick the best audio-only format from `get_song`'s `streamingData`.

use serde_json::Value;

/// Best audio-only URL (highest bitrate), or None.
pub fn pick_audio_url(song: &Value) -> Option<(String, u32)> {
    let data = song.get("streamingData")?;
    let mut best: Option<(String, u32)> = None;
    for fmt in data.get("adaptiveFormats").and_then(Value::as_array).map(|a| a.as_slice()).unwrap_or(&[]) {
        let mime = fmt.get("mimeType").and_then(Value::as_str).unwrap_or("");
        if !mime.starts_with("audio/") {
            continue;
        }
        let bitrate = fmt.get("bitrate").and_then(Value::as_u64).unwrap_or(0) as u32;
        let url = fmt.get("url").and_then(Value::as_str).unwrap_or("");
        if url.is_empty() {
            continue;
        }
        let better = match &best {
            None => true,
            Some((_, b)) => bitrate > *b,
        };
        if better {
            best = Some((url.to_string(), bitrate));
        }
    }
    // fall back to formats[]
    if best.is_none() {
        for fmt in data.get("formats").and_then(Value::as_array).map(|a| a.as_slice()).unwrap_or(&[]) {
            let mime = fmt.get("mimeType").and_then(Value::as_str).unwrap_or("");
            if !mime.starts_with("audio/") {
                continue;
            }
            let bitrate = fmt.get("bitrate").and_then(Value::as_u64).unwrap_or(0) as u32;
            let url = fmt.get("url").and_then(Value::as_str).unwrap_or("");
            if !url.is_empty() {
                best = Some((url.to_string(), bitrate));
                break;
            }
        }
    }
    best
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
            {"mimeType":"audio/webm; codecs=\"opus\"","bitrate":160000,"url":"hi"}
        ]}});
        assert_eq!(pick_audio_url(&song).unwrap().0, "hi");
        assert_eq!(pick_audio_url(&serde_json::json!({})), None);
    }
}
