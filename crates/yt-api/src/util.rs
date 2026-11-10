//! Small helpers: durations, thumbnails, ids, bridge shims.

/// Shim for old sf_api::WebApiLimits used by bridges (YTMusic has no 429 quota).
pub use crate::http::WebApiLimits as WebApiLimitsShim;

/// Station seed helper stub (Spotify station URIs have no YTMusic equivalent).
pub fn station_seed(text: &str) -> Option<String> {
    if text.starts_with("ytmusic:") { Some(text.to_string()) } else { None }
}

pub fn uri_kind(uri: &str) -> Option<&'static str> {
    crate::map::uri_kind(uri)
}

pub fn uri_id(uri: &str) -> Option<String> {
    crate::map::uri_id(uri)
}

/// Legacy `open_spotify_url` entry point kept for bridge compat.
pub fn open_spotify_url(uri: &str) -> Option<String> {
    crate::map::open_url(uri)
}

/// "3:45" / "1:02:03" / "245" (seconds) / "25 min" / "1 hr 5 min" → milliseconds.
/// Unparseable → 0.
pub fn duration_ms(value: &str) -> i64 {
    let value = value.trim();
    if value.is_empty() {
        return 0;
    }
    if let Ok(secs) = value.parse::<i64>() {
        return secs.saturating_mul(1000);
    }
    // Длительность выпуска: «25 min», «1 hr 5 min», «45 sec» (число идёт перед единицей)
    let words = value.split_whitespace().collect::<Vec<_>>();
    if words.len() >= 2
        && words
            .iter()
            .any(|w| *w == "min" || *w == "minutes" || *w == "hr" || *w == "hours" || *w == "sec" || *w == "seconds")
    {
        let mut total: i64 = 0;
        let mut pending: Option<i64> = None;
        for word in words {
            match word {
                "hr" | "hour" | "hours" => {
                    if let Some(n) = pending.take() {
                        total += n.saturating_mul(3_600_000);
                    }
                }
                "min" | "minute" | "minutes" => {
                    if let Some(n) = pending.take() {
                        total += n.saturating_mul(60_000);
                    }
                }
                "sec" | "second" | "seconds" => {
                    if let Some(n) = pending.take() {
                        total += n.saturating_mul(1_000);
                    }
                }
                _ => {
                    if let Ok(n) = word.parse::<i64>() {
                        pending = Some(n);
                    }
                }
            }
        }
        return total;
    }
    let parts: Vec<&str> = value.split(':').collect();
    let mut total: i64 = 0;
    let mut factor: i64 = 1;
    for part in parts.iter().rev() {
        let Ok(n) = part.trim().parse::<i64>() else {
            return 0;
        };
        total += n * factor;
        factor *= 60;
    }
    total.saturating_mul(1000)
}

/// ytmusicapi thumbnails: ascending size, last is largest.
/// Return the largest at least `px` wide, else the largest.
pub fn pick_thumbnail(thumbnails: &[serde_json::Value], px: u32) -> String {
    if thumbnails.is_empty() {
        return String::new();
    }
    fn url(v: &serde_json::Value) -> &str { v.get("url").and_then(|u| u.as_str()).unwrap_or("") }
    fn width(v: &serde_json::Value) -> u32 { v.get("width").and_then(|w| w.as_u64()).unwrap_or(0) as u32 }
    // Exact match is rare (URLs are i.ytimg.com, sizes vary) — pick closest above target.
    let mut best: Option<&serde_json::Value> = None;
    for t in thumbnails {
        match best {
            None => best = Some(t),
            Some(b) => {
                let (bw, tw) = (width(b), width(t));
                let better = match (bw >= px, tw >= px) {
                    (true, true) => tw < bw,
                    (false, true) => true,
                    (true, false) => false,
                    (false, false) => tw > bw,
                };
                if better {
                    best = Some(t);
                }
            }
        }
    }
    best.map(url).unwrap_or("").to_string()
}

/// Strip HTML tags (descriptions contain them).
pub fn strip_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for c in text.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.trim().to_string()
}

/// Video/track id from a ytmusicapi item (videoId | browseId | playlistId | length+videoId).
pub fn video_id(item: &serde_json::Value) -> String {
    for key in ["videoId", "browseId", "playlistId", "channelId"] {
        if let Some(s) = item.get(key).and_then(|v| v.as_str()) {
            if !s.is_empty() {
                return s.to_string();
            }
        }
    }
    // get_song / get_watch_playlist embed it under videoDetails
    item.get("videoDetails")
        .and_then(|v| v.get("videoId"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duration_parses_hms() {
        assert_eq!(duration_ms("3:45"), 225_000);
        assert_eq!(duration_ms("1:02:03"), 3_723_000);
        assert_eq!(duration_ms("245"), 245_000);
        assert_eq!(duration_ms(""), 0);
        assert_eq!(duration_ms("x"), 0);
    }

    #[test]
    fn duration_parses_podcast_lengths() {
        assert_eq!(duration_ms("25 min"), 1_500_000);
        assert_eq!(duration_ms("1 hr 5 min"), 3_900_000);
        assert_eq!(duration_ms("45 sec"), 45_000);
        assert_eq!(duration_ms("2 minutes"), 120_000);
    }
    #[test]
    fn thumbnail_prefers_smallest_sufficient() {
        let t = vec![
            serde_json::json!({"url":"lg","width":1080}),
            serde_json::json!({"url":"sm","width":60}),
            serde_json::json!({"url":"md","width":300}),
        ];
        assert_eq!(pick_thumbnail(&t, 60), "sm");
        assert_eq!(pick_thumbnail(&t, 100), "md");
        assert_eq!(pick_thumbnail(&t, 500), "lg");
        assert_eq!(pick_thumbnail(&[], 100), "");
    }
    #[test]
    fn strip_html_removes_tags() {
        assert_eq!(strip_html("<p>Hi <b>there</b></p>"), "Hi there");
    }
}
