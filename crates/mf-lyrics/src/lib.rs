//! Synced lyrics — a port of lyrics_sources.py from yamusic-md3 (+ the cache from lyrics.py).
//!
//! The source chain (fetch): Spotify (the ExtraLrc external source — first, as in spotifast; SYLLABLE_SYNCED —
//! word by word from the response's syllables, LINE_SYNCED — line by line) → NetEase YRC
//! (word by word) → Musixmatch richsync (word by word) → LRCLIB (lines) → Musixmatch subtitles → NetEase LRC.
//! For line-by-line lyrics the word times are interpolated. The result and cache format (JSON) uses Python's keys.
pub mod lrc;
pub mod sources;
pub mod text;

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use futures::future::BoxFuture;
use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize, Serializer};
use sha1::{Digest, Sha1};

use crate::lrc::{add_gaps, build_from_timed_lines, group_syllables, lines_from_lrc, parse_yrc, round3};
use crate::text::{is_meta_line, is_placeholder};

/// 4 — external LRC at the end of the chain; 5 — NetEase "no lyrics" placeholders are skipped;
/// 6 — LRCLIB: a fuzzy title and the markup most records agree on;
/// 7 — Spotify first, the track id is Spotify's base62;
/// 8 — Spotify SYLLABLE_SYNCED: word times from the response's syllables instead of interpolation
pub const CACHE_VERSION: u32 = 9;
/// "Not found" is rechecked after 6 hours
pub const NOT_FOUND_TTL_SECS: u64 = 6 * 3600;
/// HTTP timeout of the sources, in seconds (2 retries on 5xx/network with a 0.7·(n+1) s pause)
pub const TIMEOUT_SECS: u64 = 8;
/// Silence of ≥ 4.5 s — a "• • •" insertion (gap)
pub const GAP_MIN: f64 = 4.5;
pub const USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36";

/// What we look for. title already carries the version: "Title (Version)" if there is one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LyricsQuery {
    pub track_id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    /// the track's duration in seconds
    pub duration: f64,
}

/// A word: {"x": text (with a trailing space), "s": start, "e": end, "p": [[characters, s, e], …]?}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Word {
    pub x: String,
    pub s: f64,
    pub e: f64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub p: Option<Vec<(u32, f64, f64)>>,
}

/// A lyrics line {"t","e","w","i"} or a pause {"gap": true, "t", "e"} — the order of variants matters for untagged.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Line {
    Gap { gap: bool, t: f64, e: f64 },
    Words { t: f64, e: f64, w: Vec<Word>, i: bool },
}

impl Line {
    pub fn words(t: f64, e: f64, w: Vec<Word>, i: bool) -> Self {
        Self::Words { t, e, w, i }
    }

    /// "t" — start
    pub fn start(&self) -> f64 {
        match self {
            Self::Gap { t, .. } | Self::Words { t, .. } => *t,
        }
    }

    /// "e" — end
    pub fn end(&self) -> f64 {
        match self {
            Self::Gap { e, .. } | Self::Words { e, .. } => *e,
        }
    }

    /// The line's text (words in a row); empty for a pause
    pub fn text(&self) -> String {
        match self {
            Self::Gap { .. } => String::new(),
            Self::Words { w, .. } => w.iter().map(|w| w.x.as_str()).collect(),
        }
    }
}

/// The result of fetch and the contents of the cache file:
/// {"ok": true, "source": "Spotify"|"NetEase"|"Musixmatch"|"LRCLIB", "kind": "word"|"line", "lines": [...]}
/// or {"ok": false, "error": "not found", "details": ["source: error", …]}.
/// Serialization is manual, with exactly these two forms (like a dict in Python).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct LyricsResult {
    pub ok: bool,
    pub source: String,
    pub kind: String,
    pub lines: Vec<Line>,
    pub error: Option<String>,
    pub details: Vec<String>,
}

impl LyricsResult {
    pub fn found(source: &str, kind: &str, lines: Vec<Line>) -> Self {
        Self { ok: true, source: source.into(), kind: kind.into(), lines, ..Default::default() }
    }

    pub fn not_found(details: Vec<String>) -> Self {
        Self { ok: false, error: Some("not found".into()), details, ..Default::default() }
    }

    /// JSON like Python's json.dumps(result, ensure_ascii=False) (with ", " and ": " separators)
    pub fn to_python_json(&self) -> String {
        let mut out = Vec::new();
        let mut ser = serde_json::Serializer::with_formatter(&mut out, PyFormatter);
        // writing into a Vec does not panic
        let _ = self.serialize(&mut ser);
        String::from_utf8(out).unwrap_or_default()
    }
}

impl Serialize for LyricsResult {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("ok", &self.ok)?;
        if self.ok {
            map.serialize_entry("source", &self.source)?;
            map.serialize_entry("kind", &self.kind)?;
            map.serialize_entry("lines", &self.lines)?;
        } else {
            map.serialize_entry("error", self.error.as_deref().unwrap_or("not found"))?;
            map.serialize_entry("details", &self.details)?;
        }
        map.end()
    }
}

/// json.dumps' default separators
struct PyFormatter;

impl serde_json::ser::Formatter for PyFormatter {
    fn begin_array_value<W: ?Sized + io::Write>(&mut self, writer: &mut W, first: bool) -> io::Result<()> {
        if first { Ok(()) } else { writer.write_all(b", ") }
    }

    fn begin_object_key<W: ?Sized + io::Write>(&mut self, writer: &mut W, first: bool) -> io::Result<()> {
        if first { Ok(()) } else { writer.write_all(b", ") }
    }

    fn begin_object_value<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        writer.write_all(b": ")
    }
}

/// An external source — the JSON of Spotify's `color-lyrics` response (a closure from sf-services over
/// PlayerHandle::lyrics_json). Ok(None) — there is no text; Err — the source did not answer (such a "not found" is not cached).
pub type ExtraLrc = Arc<dyn Fn() -> BoxFuture<'static, Result<Option<serde_json::Value>, String>> + Send + Sync>;

/// The ExtraLrc source's label in the result (`source`).
pub const EXTRA_SOURCE: &str = "Spotify";

/// Spotify's milliseconds: a string or a number
fn ms(value: &serde_json::Value) -> Option<u64> {
    value.as_str().and_then(|s| s.trim().parse().ok()).or_else(|| value.as_u64())
}

/// A Spotify syllable {"startTimeMs", "endTimeMs", "words"} → (text, start, end) in seconds
fn syllable(value: &serde_json::Value) -> Option<(String, f64, f64)> {
    let start = value.get("startTimeMs").and_then(ms)?;
    let end = value.get("endTimeMs").and_then(ms).unwrap_or(start);
    let words = value.get("words")?.as_str()?;
    Some((words.to_owned(), round3(start as f64 / 1000.0), round3(end as f64 / 1000.0)))
}

/// Spotify's `color-lyrics` response with SYLLABLE_SYNCED → lines with real word times from `syllables` (i = false).
/// None — a different syncType, or at least one sung line has no syllable markup: then the line-by-line
/// `lrc_from_spotify` with interpolation works, as before. "♪" lines and empty tags are skipped.
pub fn lines_from_spotify(json: &serde_json::Value) -> Option<Vec<Line>> {
    let lyrics = json.get("lyrics")?;
    if lyrics.get("syncType").and_then(|v| v.as_str()) != Some("SYLLABLE_SYNCED") {
        return None;
    }
    let mut out = Vec::new();
    for line in lyrics.get("lines")?.as_array()? {
        let Some(at_ms) = line.get("startTimeMs").and_then(ms) else { continue };
        let words = line.get("words").and_then(|v| v.as_str()).unwrap_or("");
        let text = words.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() || text == "\u{266a}" || is_meta_line(&text) {
            continue;
        }
        let syllables: Vec<(String, f64, f64)> = line
            .get("syllables")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(syllable).collect())
            .unwrap_or_default();
        // incomplete markup — line-by-line lyrics beat a lost line
        if syllables.is_empty() {
            return None;
        }
        let words = group_syllables(&syllables);
        let Some(last) = words.last() else { continue };
        let t = round3(at_ms as f64 / 1000.0);
        out.push(Line::words(t, round3(t.max(last.e)), words, false));
    }
    (!out.is_empty()).then_some(out)
}

/// Spotify's `color-lyrics` response (JSON {"lyrics": {"syncType", "lines": [{"startTimeMs", "words"}]}}) → LRC
/// line by line, `[mm:ss.mmm]text`. A port of lyrics::from_spotify from spotifast. "♪" lines and empty ones stay
/// empty tags — the end of the previous line is computed from them. Text without times (UNSYNCED) — None:
/// the remaining sources then look for synced lyrics.
pub fn lrc_from_spotify(json: &serde_json::Value) -> Option<String> {
    let lyrics = json.get("lyrics")?;
    let sync = lyrics.get("syncType").and_then(|v| v.as_str()).unwrap_or("");
    if sync != "LINE_SYNCED" && sync != "SYLLABLE_SYNCED" {
        return None;
    }
    let mut timed: Vec<(u64, String)> = Vec::new();
    for line in lyrics.get("lines")?.as_array()? {
        let Some(at_ms) = line.get("startTimeMs").and_then(ms) else {
            continue;
        };
        let words = line.get("words").and_then(|v| v.as_str()).unwrap_or("");
        // line breaks inside must not break the LRC
        let text = words.split_whitespace().collect::<Vec<_>>().join(" ");
        let text = if text == "\u{266a}" { String::new() } else { text };
        timed.push((at_ms, text));
    }
    let sung: Vec<&(u64, String)> = timed.iter().filter(|(_, text)| !text.is_empty()).collect();
    // all tags are zero — there is no real synchronization
    if sung.is_empty() || (sung.len() > 1 && sung.iter().all(|(at, _)| *at == 0)) {
        return None;
    }
    let mut out = String::new();
    for (at_ms, text) in timed {
        out.push_str(&format!("[{:02}:{:02}.{:03}]{text}\n", at_ms / 60_000, at_ms / 1000 % 60, at_ms % 1000));
    }
    Some(out)
}

/// The cache file name: sha1(f"{CACHE_VERSION}|{track_id}") hex + ".json"
pub fn cache_file_name(track_id: &str) -> String {
    cache_file_name_v(CACHE_VERSION, track_id)
}

/// The same for an explicit version (the Python references were computed for version 5)
pub fn cache_file_name_v(version: u32, track_id: &str) -> String {
    let key = Sha1::digest(format!("{version}|{track_id}").as_bytes());
    format!("{}.json", hex::encode(key))
}

/// A source error → into details (like attempt in Python)
fn attempt<T>(errors: &mut Vec<String>, name: &str, r: Result<T, String>) -> Option<T> {
    match r {
        Ok(v) => Some(v),
        Err(e) => {
            tracing::debug!("lyrics: {name} failed: {e}");
            errors.push(format!("{name}: {e}"));
            None
        }
    }
}

/// Where to take the HTTP client from; Err — the network is closed (for example wrong proxy settings).
type ClientSource = std::sync::Arc<dyn Fn() -> Result<reqwest::Client, String> + Send + Sync>;

/// A lyrics source with an on-disk cache.
#[derive(Clone)]
pub struct LyricsFetcher {
    cache_dir: PathBuf,
    http: ClientSource,
}

impl LyricsFetcher {
    /// `cache_dir` — dirs::cache_subdir("lyrics"); mxm_token.json lives in it too.
    pub fn new(cache_dir: PathBuf) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(TIMEOUT_SECS))
            .build()
            .unwrap_or_default();
        Self::with_client(cache_dir, http)
    }

    /// With your own HTTP client (the sources set headers and a timeout on every request anyway).
    pub fn with_client(cache_dir: PathBuf, http: reqwest::Client) -> Self {
        Self::with_source(cache_dir, move || Ok(http.clone()))
    }

    /// The client is taken anew for every search: a proxy change in the app applies at once.
    pub fn with_source(
        cache_dir: PathBuf,
        source: impl Fn() -> Result<reqwest::Client, String> + Send + Sync + 'static,
    ) -> Self {
        if let Err(e) = std::fs::create_dir_all(&cache_dir) {
            tracing::warn!("lyrics: cache directory {} not created: {e}", cache_dir.display());
        }
        Self { cache_dir, http: std::sync::Arc::new(source) }
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// <cache_dir>/<sha1>.json
    pub fn cache_path(&self, track_id: &str) -> PathBuf {
        self.cache_dir.join(cache_file_name(track_id))
    }

    /// A fresh cache entry: ok — forever, not found — while younger than NOT_FOUND_TTL_SECS.
    pub async fn read_cache(&self, track_id: &str) -> Option<LyricsResult> {
        let path = self.cache_path(track_id);
        let raw = tokio::fs::read_to_string(&path).await.ok()?;
        let cached: LyricsResult = serde_json::from_str(&raw).ok()?;
        if cached.ok {
            return Some(cached);
        }
        let mtime = tokio::fs::metadata(&path).await.ok()?.modified().ok()?;
        // an mtime from the future is fresh too, as in Python
        let age = SystemTime::now().duration_since(mtime).unwrap_or_default();
        (age < Duration::from_secs(NOT_FOUND_TTL_SECS)).then_some(cached)
    }

    /// Writing via .part + rename
    pub async fn write_cache(&self, track_id: &str, result: &LyricsResult) -> io::Result<()> {
        let path = self.cache_path(track_id);
        tokio::fs::create_dir_all(&self.cache_dir).await?;
        let tmp = path.with_extension("part");
        tokio::fs::write(&tmp, result.to_python_json()).await?;
        tokio::fs::rename(&tmp, &path).await
    }

    /// Word-level result from the sources that have it, without lines-only fallbacks.
    /// Called after Spotify returned line lyrics: the caller waits a little and, if this is
    /// slow, keeps the line result and lets a background task fill the cache.
    async fn word_result(&self, query: &LyricsQuery) -> Option<LyricsResult> {
        let http = (self.http)().ok()?;
        if let Ok(Some(song)) = sources::ne_search(&http, query).await {
            if let Some(id) = song.get("id") {
                if let Ok(data) = sources::ne_lyric_by_id(&http, &py_id(id)).await {
                    // no YRC is not an exit from the function: we try Musixmatch next
                    let yrc = data.get("yrc").and_then(|y| y.get("lyric")).and_then(|v| v.as_str()).unwrap_or("");
                    if !yrc.is_empty() {
                        let lines = parse_yrc(yrc);
                        let texts: Vec<String> = lines.iter().map(Line::text).collect();
                        if !lines.is_empty() && !is_placeholder(texts.iter().map(String::as_str)) {
                            return Some(LyricsResult::found("NetEase", "word", lines));
                        }
                    }
                }
            }
        }
        let token = sources::mxm_token(&http, &self.cache_dir, "").await.ok().flatten().filter(|t| !t.is_empty())?;
        match sources::mxm_fetch(&http, query, &token).await {
            Ok(Some(("word", lines))) => Some(LyricsResult::found("Musixmatch", "word", lines)),
            _ => None,
        }
    }

    /// Like _fetch_cached in lyrics.py: the file <cache_dir>/<sha1(f"{CACHE_VERSION}|{track_id}")>.json;
    /// ok — forever, not found — NOT_FOUND_TTL_SECS by mtime. What is found goes through add_gaps(lines, duration).
    /// A result with non-empty details (the source answered with an error) is not cached. Writing — via .part + rename.
    pub async fn fetch_cached(&self, query: &LyricsQuery, extra: Option<ExtraLrc>) -> LyricsResult {
        if let Some(cached) = self.read_cache(&query.track_id).await {
            return cached;
        }
        let mut result = self.fetch(query, "", extra).await;
        if result.ok && result.source == EXTRA_SOURCE && result.kind == "line" {
            // Spotify knew the lines immediately; a word-level source makes the fill non-linear.
            // Wait a little, then keep the line and finish the upgrade in the background.
            // a single request: on timeout it is not restarted, but finishes in the background
            let me = self.clone();
            let q = query.clone();
            let mut task = tokio::spawn(async move { me.word_result(&q).await });
            match tokio::time::timeout(Duration::from_millis(1500), &mut task).await {
                Ok(Ok(Some(word))) => result = word,
                Ok(_) => {}
                Err(_) => {
                    let me = self.clone();
                    let query = query.clone();
                    tokio::spawn(async move {
                        let Ok(Some(mut word)) = task.await else { return };
                        word.lines = add_gaps(std::mem::take(&mut word.lines), query.duration);
                        if let Err(e) = me.write_cache(&query.track_id, &word).await {
                            tracing::warn!("lyrics: word upgrade {} not cached: {e}", query.track_id);
                        }
                    });
                }
            }
        }
        if result.ok {
            result.lines = add_gaps(std::mem::take(&mut result.lines), query.duration);
        }
        // A source with an error (503, timeout) might have known the lyrics — such a "not found" is not cached
        if (result.ok || result.details.is_empty())
            && let Err(e) = self.write_cache(&query.track_id, &result).await
        {
            tracing::warn!("lyrics: cache {} not written: {e}", query.track_id);
        }
        result
    }

    /// The source chain without a cache (fetch in lyrics_sources.py); mxm_user_token — empty means an anonymous token.
    pub async fn fetch(&self, query: &LyricsQuery, mxm_user_token: &str, extra: Option<ExtraLrc>) -> LyricsResult {
        let duration = query.duration;
        let mut errors = Vec::new();

        // 0. Spotify — first: this is the text of this very recording. SYLLABLE_SYNCED gives word times;
        // LINE_SYNCED is returned at once so the text does not wait for the slow sources. If word
        // times (NetEase/Musixmatch) are available alongside, fetch_cached will pull them in within 1.5 s and cache them;
        // the slow source is loaded in the background.
        if let Some(extra) = extra {
            let json = attempt(&mut errors, "spotify", extra().await).flatten();
            if let Some(json) = json {
                if let Some(lines) = lines_from_spotify(&json) {
                    return LyricsResult::found(EXTRA_SOURCE, "word", lines);
                }
                if let Some(lrc) = lrc_from_spotify(&json).filter(|l| !l.is_empty()) {
                    let lines = build_from_timed_lines(&lines_from_lrc(&lrc), duration);
                    if !lines.is_empty() {
                        return LyricsResult::found(EXTRA_SOURCE, "line", lines);
                    }
                }
            }
        }

        let http = match (self.http)() {
            Ok(http) => http,
            Err(e) => {
                errors.push(format!("network: {e}"));
                return LyricsResult::not_found(errors);
            }
        };
        let http = &http;

        // 1. NetEase YRC (word by word)
        let mut ne_data = None;
        if let Some(song) = attempt(&mut errors, "netease-search", sources::ne_search(http, query).await).flatten() {
            tracing::debug!("lyrics: netease match {} {}", song["id"], song["name"]);
            let lyric = match song.get("id") {
                Some(id) => sources::ne_lyric_by_id(http, &py_id(id)).await,
                None => Err("'id'".to_owned()),
            };
            ne_data = attempt(&mut errors, "netease-lyric", lyric);
            let yrc = ne_data.as_ref().and_then(|d| d.get("yrc")?.get("lyric")?.as_str()).unwrap_or("");
            if !yrc.is_empty() {
                let lines = parse_yrc(yrc);
                let texts: Vec<String> = lines.iter().map(Line::text).collect();
                if !lines.is_empty() && is_placeholder(texts.iter().map(String::as_str)) {
                    tracing::debug!("lyrics: netease yrc: placeholder, skip");
                } else if !lines.is_empty() {
                    return LyricsResult::found("NetEase", "word", lines);
                }
            }
        }

        // 2. Musixmatch richsync (word by word) — also gives a line-by-line fallback
        let mut mxm_line = Vec::new();
        let token = sources::mxm_token(http, &self.cache_dir, mxm_user_token).await;
        let token = attempt(&mut errors, "musixmatch-token", token).flatten().filter(|t| !t.is_empty());
        if let Some(token) = token {
            match attempt(&mut errors, "musixmatch", sources::mxm_fetch(http, query, &token).await).flatten() {
                Some(("word", lines)) => return LyricsResult::found("Musixmatch", "word", lines),
                Some((_, lines)) => mxm_line = lines,
                None => {}
            }
        }

        // 3. LRCLIB (line by line, interpolation)
        let lrc = attempt(&mut errors, "lrclib", sources::lrclib_fetch(http, query).await).flatten();
        if let Some(lrc) = lrc.filter(|l| !l.is_empty()) {
            let lines = build_from_timed_lines(&lines_from_lrc(&lrc), duration);
            if !lines.is_empty() {
                return LyricsResult::found("LRCLIB", "line", lines);
            }
        }

        // 4. Musixmatch subtitles
        if !mxm_line.is_empty() {
            return LyricsResult::found("Musixmatch", "line", mxm_line);
        }

        // 5. NetEase LRC
        let ne_lrc = ne_data.as_ref().and_then(|d| d.get("lrc")?.get("lyric")?.as_str()).unwrap_or("");
        if !ne_lrc.is_empty() {
            let pairs = lines_from_lrc(ne_lrc);
            if !is_placeholder(pairs.iter().map(|(_, t)| t.as_str())) {
                let lines = build_from_timed_lines(&pairs, duration);
                if !lines.is_empty() {
                    return LyricsResult::found("NetEase", "line", lines);
                }
            }
        }

        LyricsResult::not_found(errors)
    }
}

/// str(song["id"]) Python
fn py_id(id: &serde_json::Value) -> String {
    match id {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> LyricsResult {
        LyricsResult::found(
            "Spotify",
            "line",
            vec![
                Line::Gap { gap: true, t: 0.0, e: 4.85 },
                Line::words(
                    5.0,
                    6.5,
                    vec![Word { x: "one ".into(), s: 5.0, e: 5.5, p: Some(vec![(1, 5.0, 5.25), (2, 5.25, 5.5)]) }],
                    true,
                ),
            ],
        )
    }

    #[test]
    fn result_json_is_python_shaped() {
        assert_eq!(
            sample().to_python_json(),
            r#"{"ok": true, "source": "Spotify", "kind": "line", "lines": [{"gap": true, "t": 0.0, "e": 4.85}, {"t": 5.0, "e": 6.5, "w": [{"x": "one ", "s": 5.0, "e": 5.5, "p": [[1, 5.0, 5.25], [2, 5.25, 5.5]]}], "i": true}]}"#
        );
        assert_eq!(
            LyricsResult::not_found(vec![]).to_python_json(),
            r#"{"ok": false, "error": "not found", "details": []}"#
        );
    }

    #[test]
    fn result_json_roundtrips() {
        let r = sample();
        assert_eq!(serde_json::from_str::<LyricsResult>(&r.to_python_json()).unwrap(), r);
        let nf = LyricsResult::not_found(vec!["lrclib: HTTP Error 503: Service Unavailable".into()]);
        assert_eq!(serde_json::from_str::<LyricsResult>(&serde_json::to_string(&nf).unwrap()).unwrap(), nf);
    }

    #[test]
    fn cache_key_matches_python() {
        // hashlib.sha1(b"5|12345").hexdigest()
        assert_eq!(cache_file_name_v(5, "12345"), "e13466f72878a42f8fe435c7ee2b441ee93e2d57.json");
        assert_ne!(cache_file_name("12345"), cache_file_name_v(5, "12345"));
    }

    #[tokio::test]
    async fn cache_is_read_back_and_not_found_expires() {
        let dir = std::env::temp_dir().join(format!("mf-lyrics-test-{}", std::process::id()));
        let fetcher = LyricsFetcher::new(dir.clone());
        fetcher.write_cache("1", &sample()).await.unwrap();
        assert_eq!(fetcher.read_cache("1").await, Some(sample()));
        assert!(!dir.join(cache_file_name("1")).with_extension("part").exists());

        let nf = LyricsResult::not_found(vec![]);
        fetcher.write_cache("2", &nf).await.unwrap();
        assert_eq!(fetcher.read_cache("2").await, Some(nf));
        // a stale "not found" is rechecked
        let old = SystemTime::now() - Duration::from_secs(NOT_FOUND_TTL_SECS + 60);
        std::fs::File::options().write(true).open(fetcher.cache_path("2")).unwrap().set_modified(old).unwrap();
        assert_eq!(fetcher.read_cache("2").await, None);
        assert_eq!(fetcher.read_cache("missing").await, None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn a_closed_network_is_not_bypassed_and_not_cached() {
        let dir = std::env::temp_dir().join(format!("mf-lyrics-test-closed-{}", std::process::id()));
        let fetcher = LyricsFetcher::with_source(dir.clone(), || Err("proxy is misconfigured".into()));
        let query = LyricsQuery { track_id: "closed".into(), title: "t".into(), artist: "a".into(), ..Default::default() };
        let result = fetcher.fetch_cached(&query, None).await;
        assert!(!result.ok);
        assert!(result.details.iter().any(|detail| detail.contains("proxy")));
        assert_eq!(fetcher.read_cache("closed").await, None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn cached_result_skips_sources() {
        let dir = std::env::temp_dir().join(format!("mf-lyrics-test-cached-{}", std::process::id()));
        let fetcher = LyricsFetcher::new(dir.clone());
        fetcher.write_cache("42", &sample()).await.unwrap();
        let query = LyricsQuery { track_id: "42".into(), title: "x".into(), ..Default::default() };
        assert_eq!(fetcher.fetch_cached(&query, None).await, sample());
        let _ = std::fs::remove_dir_all(dir);
    }

    fn color_lyrics(sync: &str) -> serde_json::Value {
        serde_json::json!({"lyrics": {"syncType": sync, "lines": [
            {"startTimeMs": "0", "words": "♪", "syllables": []},
            {"startTimeMs": "17120", "words": "Hello darkness, my old friend"},
            {"startTimeMs": 21860, "words": "  I've come\nto talk  "},
            {"startTimeMs": "27010", "words": ""},
            {"startTimeMs": "3723456", "words": "end"},
            {"words": "no time"}
        ]}})
    }

    #[test]
    fn spotify_lines_become_lrc() {
        let lrc = lrc_from_spotify(&color_lyrics("LINE_SYNCED")).unwrap();
        assert_eq!(
            lrc,
            "[00:00.000]\n[00:17.120]Hello darkness, my old friend\n[00:21.860]I've come to talk\n[00:27.010]\n\
             [62:03.456]end\n"
        );
        let pairs = lrc::lines_from_lrc(&lrc);
        assert_eq!(pairs[1], (17.12, "Hello darkness, my old friend".to_owned()));
        assert_eq!(pairs[4], (3723.456, "end".to_owned()));
        // empty tags bound the previous line
        let lines = lrc::build_from_timed_lines(&pairs, 4000.0);
        assert_eq!(lines.len(), 3);
        assert!(lines[1].end() <= 26.96 + 1e-9);

        assert_eq!(lrc_from_spotify(&color_lyrics("UNSYNCED")), None);
        assert!(lrc_from_spotify(&color_lyrics("SYLLABLE_SYNCED")).is_some());
        assert_eq!(lrc_from_spotify(&serde_json::json!({})), None);
        assert_eq!(lrc_from_spotify(&serde_json::json!({"lyrics": {"syncType": "LINE_SYNCED", "lines": []}})), None);
        let zeros = serde_json::json!({"lyrics": {"syncType": "LINE_SYNCED", "lines": [
            {"startTimeMs": "0", "words": "one"}, {"startTimeMs": "0", "words": "two"}]}});
        assert_eq!(lrc_from_spotify(&zeros), None);
    }

    #[tokio::test]
    async fn spotify_comes_first() {
        let dir = std::env::temp_dir().join(format!("mf-lyrics-test-spotify-{}", std::process::id()));
        let fetcher = LyricsFetcher::new(dir.clone());
        let json = color_lyrics("LINE_SYNCED");
        let extra: ExtraLrc = Arc::new(move || {
            let json = json.clone();
            Box::pin(async move { Ok(Some(json)) })
        });
        let query = LyricsQuery { track_id: "4uLU6hMCjMI75M1A2tKUQC".into(), title: "x".into(), duration: 300.0, ..Default::default() };
        let result = fetcher.fetch_cached(&query, Some(extra)).await;
        assert!(result.ok);
        assert_eq!(result.source, EXTRA_SOURCE);
        assert_eq!(result.kind, "line");
        assert_eq!(result.lines.iter().filter(|l| !l.text().is_empty()).count(), 3);
        // what was found is written to the current version's cache
        assert_eq!(fetcher.read_cache(&query.track_id).await, Some(result));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// SYLLABLE_SYNCED: every sung line has syllables with their own times.
    fn color_lyrics_syllables() -> serde_json::Value {
        serde_json::json!({"lyrics": {"syncType": "SYLLABLE_SYNCED", "lines": [
            {"startTimeMs": "0", "words": "♪", "syllables": []},
            {"startTimeMs": "17120", "words": "Hello darkness", "syllables": [
                {"startTimeMs": "17120", "endTimeMs": "17620", "words": "Hel"},
                {"startTimeMs": "17620", "endTimeMs": "18140", "words": "lo "},
                {"startTimeMs": "18140", "endTimeMs": "19100", "words": "dark"},
                {"startTimeMs": "19100", "endTimeMs": "19700", "words": "ness"}]},
            {"startTimeMs": "21860", "words": "my old friend", "syllables": [
                {"startTimeMs": "21860", "endTimeMs": "22100", "words": "my "},
                {"startTimeMs": "22100", "endTimeMs": "22600", "words": "old "},
                {"startTimeMs": "22600", "endTimeMs": "23260", "words": "friend"}]},
            {"startTimeMs": "27010", "words": "", "syllables": []}
        ]}})
    }

    #[test]
    fn syllable_synced_lines_keep_word_timing() {
        let lines = lines_from_spotify(&color_lyrics_syllables()).unwrap();
        assert_eq!(lines.len(), 2, "the \"♪\" line and the empty tag are not lines");
        let Line::Words { t, e, w, i, .. } = &lines[0] else { panic!("expected a line with words") };
        assert!(!i, "the word times are real, not interpolated");
        assert_eq!((*t, *e), (17.12, 19.7));
        assert_eq!(w.iter().map(|w| w.x.as_str()).collect::<Vec<_>>(), vec!["Hello ", "darkness"]);
        // the syllables of one word stay inside it: "Hel"+"lo " and "dark"+"ness"
        assert_eq!(w[0], Word { x: "Hello ".into(), s: 17.12, e: 18.14, p: Some(vec![(3, 17.12, 17.62), (2, 17.62, 18.14)]) });
        assert_eq!(w[1], Word { x: "darkness".into(), s: 18.14, e: 19.7, p: Some(vec![(4, 18.14, 19.1), (4, 19.1, 19.7)]) });
        assert_eq!(lines[1].end(), 23.26);
        assert_eq!(lines[1].text(), "my old friend");
    }

    #[tokio::test]
    async fn synthetic_spotify_lines_win_over_interpolation() {
        let dir = std::env::temp_dir().join(format!("mf-lyrics-test-syllables-{}", std::process::id()));
        let fetcher = LyricsFetcher::new(dir.clone());
        let json = color_lyrics_syllables();
        let extra: ExtraLrc = Arc::new(move || {
            let json = json.clone();
            Box::pin(async move { Ok(Some(json)) })
        });
        let query = LyricsQuery { track_id: "syllables".into(), title: "x".into(), duration: 300.0, ..Default::default() };
        let result = fetcher.fetch_cached(&query, Some(extra)).await;
        assert!(result.ok);
        assert_eq!((result.source.as_str(), result.kind.as_str()), (EXTRA_SOURCE, "word"));
        assert_eq!(result.lines.iter().filter(|l| !l.text().is_empty()).count(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn incomplete_syllables_fall_back_to_the_line_lrc() {
        let dir = std::env::temp_dir().join(format!("mf-lyrics-test-half-syllables-{}", std::process::id()));
        // The network is off: we check that line-by-line Spotify is not lost when the other
        // sources are unavailable (it is now the fallback, not the first).
        let fetcher = LyricsFetcher::with_source(dir.clone(), || Err("offline".into()));
        // SYLLABLE_SYNCED, but the sung line has no syllables — the line-by-line path and interpolation
        let json = serde_json::json!({"lyrics": {"syncType": "SYLLABLE_SYNCED", "lines": [
            {"startTimeMs": "1000", "words": "one two", "syllables": []},
            {"startTimeMs": "3000", "words": "three", "syllables": [
                {"startTimeMs": "3000", "endTimeMs": "3500", "words": "three"}]}
        ]}});
        assert_eq!(lines_from_spotify(&json), None);
        let extra: ExtraLrc = Arc::new(move || {
            let json = json.clone();
            Box::pin(async move { Ok(Some(json)) })
        });
        let query = LyricsQuery { track_id: "half".into(), title: "x".into(), duration: 60.0, ..Default::default() };
        let result = fetcher.fetch_cached(&query, Some(extra)).await;
        assert!(result.ok);
        assert_eq!((result.source.as_str(), result.kind.as_str()), (EXTRA_SOURCE, "line"));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// A live check of the sources: `MATERIALFY_LYRICS_LIVE=1 cargo test -p mf-lyrics live -- --nocapture`.
    #[tokio::test]
    async fn live_sources_find_a_known_song() {
        if std::env::var("MATERIALFY_LYRICS_LIVE").as_deref() != Ok("1") {
            return;
        }
        let dir = std::env::temp_dir().join("mf-lyrics-live");
        let fetcher = LyricsFetcher::new(dir);
        for (title, artist, duration) in [("Creep", "Radiohead", 238.0), ("Yesterday", "The Beatles", 125.0)] {
            let query = LyricsQuery { title: title.into(), artist: artist.into(), duration, ..Default::default() };
            let http = (fetcher.http)().unwrap();
            if let Ok(Some(song)) = sources::ne_search(&http, &query).await {
                let data = sources::ne_lyric_by_id(&http, &py_id(&song["id"])).await;
                let has = |k: &str| data.as_ref().ok().and_then(|d| d[k]["lyric"].as_str()).is_some_and(|s| !s.is_empty());
                println!("netease {} {}: yrc {} lrc {}", song["id"], song["name"], has("yrc"), has("lrc"));
            }
            let r = fetcher.fetch(&query, "", None).await;
            println!("{artist} — {title}: {} {} lines {} errors {:?}", r.source, r.kind, r.lines.len(), r.details);
            let first: Vec<String> = r.lines.iter().take(3).map(Line::text).collect();
            println!("   {}", first.join(" | "));
            assert!(r.ok);
        }
    }
}
