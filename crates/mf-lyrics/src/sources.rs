//! External sources (lyrics_sources.py). All requests use the User-Agent from crate::USER_AGENT, the TIMEOUT_SECS timeout,
//! and up to 2 retries on a network error/5xx with a 0.7·(n+1) s pause.
//!
//! - NetEase search: GET https://music.163.com/api/cloudsearch/pc?s=<clean_title primary_artist>&type=1&limit=15&offset=0
//!   (Referer: https://music.163.com); picking the best one — the ne_search rules;
//! - NetEase lyrics: POST https://interface3.music.163.com/eapi/song/lyric/v1, form params=<eapi>,
//!   Referer as above, Cookie: "os=pc; appver=2.10.13"; eapi = HEX_UPPER(AES-128-ECB-PKCS7(key "e82ckenh8dichen8",
//!   f"{path}-36cd479b6b5-{json}-36cd479b6b5-{md5(f'nobody{path}use{json}md5forencrypt')}")), path "/api/song/lyric/v1",
//!   json — compact (separators ',' ':') {"id","cp":"false","lv":"0","kv":"0","tv":"0","rv":"0","yv":"0","ytv":"0","yrv":"0","csrf_token":""};
//! - Musixmatch: https://apic-desktop.musixmatch.com/ws/1.1/, authority/cookie headers as in MXM_HEADERS, app_id web-desktop-app-v1.0;
//!   the token.get token is cached in <cache>/mxm_token.json {"token", "expires"} (6 h; null — 1 h);
//! - LRCLIB: https://lrclib.net/api/get and /api/search, the Lrclib-Client header: materialfy.
//!
//! Third-party APIs change, so JSON is read leniently (a missing field = empty), as in the Kotlin version.
use std::fmt;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aes::Aes128;
use ecb::cipher::block_padding::Pkcs7;
use ecb::cipher::{BlockEncryptMut, KeyInit};
use md5::{Digest, Md5};
use reqwest::header::{REFERER, USER_AGENT};
use serde_json::{Value, json};

use crate::lrc::{build_from_timed_lines, group_syllables, round3};
use crate::text::{clean_title, norm, primary_artist, similar, version_tags};
use crate::{Line, LyricsQuery, TIMEOUT_SECS};

const RETRIES: u32 = 2;
const NE_KEY: &[u8; 16] = b"e82ckenh8dichen8";
const NE_REFERER: &str = "https://music.163.com";
pub const NE_LYRIC_PATH: &str = "/api/song/lyric/v1";
pub const MXM_BASE: &str = "https://apic-desktop.musixmatch.com/ws/1.1/";
pub const MXM_HEADERS: [(&str, &str); 2] = [("authority", "apic-desktop.musixmatch.com"), ("cookie", "x-mxm-token-guid=")];
const MXM_APP_ID: &str = "web-desktop-app-v1.0";
const LRCLIB_HEADER: (&str, &str) = ("Lrclib-Client", "materialfy");

// ── HTTP ─────────────────────────────────────────────────────────────────────

#[derive(Debug)]
enum HttpError {
    Status(u16),
    Net(String),
    Json(String),
}

impl fmt::Display for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // like urllib.error.HTTPError
            Self::Status(code) => {
                let reason = reqwest::StatusCode::from_u16(*code).ok().and_then(|s| s.canonical_reason()).unwrap_or("");
                write!(f, "HTTP Error {code}: {reason}")
            }
            Self::Net(e) | Self::Json(e) => f.write_str(e),
        }
    }
}

impl From<HttpError> for String {
    fn from(e: HttpError) -> Self {
        e.to_string()
    }
}

/// reqwest's error text together with its causes (on its own it is too generic)
fn error_chain(e: &reqwest::Error) -> String {
    let mut out = e.to_string();
    let mut src = std::error::Error::source(e);
    while let Some(s) = src {
        out.push_str(": ");
        out.push_str(&s.to_string());
        src = s.source();
    }
    out
}

/// A request with retries on a network error and 5xx; the body is UTF-8 with bad bytes replaced.
async fn http_text(build: impl Fn() -> reqwest::RequestBuilder) -> Result<String, HttpError> {
    let mut attempt = 0;
    loop {
        let req = build().header(USER_AGENT, crate::USER_AGENT);
        let err = match req.send().await {
            Ok(resp) => {
                let status = resp.status();
                if status.is_client_error() || status.is_server_error() {
                    if status.as_u16() < 500 || attempt == RETRIES {
                        return Err(HttpError::Status(status.as_u16()));
                    }
                    HttpError::Status(status.as_u16())
                } else {
                    match resp.bytes().await {
                        Ok(body) => return Ok(String::from_utf8_lossy(&body).into_owned()),
                        Err(e) => HttpError::Net(error_chain(&e)),
                    }
                }
            }
            Err(e) => HttpError::Net(error_chain(&e)),
        };
        if attempt == RETRIES {
            return Err(err);
        }
        attempt += 1;
        tokio::time::sleep(Duration::from_secs_f64(0.7 * attempt as f64)).await;
    }
}

async fn http_json(build: impl Fn() -> reqwest::RequestBuilder) -> Result<Value, HttpError> {
    let text = http_text(build).await?;
    serde_json::from_str(&text).map_err(|e| HttpError::Json(e.to_string()))
}

fn timeout() -> Duration {
    Duration::from_secs(TIMEOUT_SECS)
}

// ── lenient JSON reading (Python semantics: x.get(k) or {}) ──────────────────

static NULL: Value = Value::Null;

fn get<'a>(v: &'a Value, key: &str) -> &'a Value {
    v.get(key).unwrap_or(&NULL)
}

/// Truthiness of a value in Python
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|x| x != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Python's str(x) for values from JSON (for request parameters)
fn py_str(v: &Value) -> String {
    match v {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn text_of(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

/// Python's float(x): a number or a string with a number
fn py_float(v: &Value, what: &str) -> Result<f64, String> {
    match v {
        Value::Number(n) => n.as_f64().ok_or_else(|| format!("bad number {what}")),
        Value::String(s) => s.trim().parse().map_err(|_| format!("could not convert string to float: '{s}'")),
        Value::Null => Err(format!("missing {what}")),
        other => Err(format!("float() argument must be a string or a real number, not {other} ({what})")),
    }
}

fn now_secs() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

// ── NetEase ──────────────────────────────────────────────────────────────────

/// The best NetEase song match (the song object) or None.
pub async fn ne_search(http: &reqwest::Client, query: &LyricsQuery) -> Result<Option<Value>, String> {
    let q = format!("{} {}", clean_title(&query.title), primary_artist(&query.artist));
    let url = "https://music.163.com/api/cloudsearch/pc";
    let d = http_json(|| {
        http.get(url)
            .query(&[("s", q.as_str()), ("type", "1"), ("limit", "15"), ("offset", "0")])
            .header(REFERER, NE_REFERER)
            .timeout(timeout())
    })
    .await?;
    let songs = get(get(&d, "result"), "songs").as_array().cloned().unwrap_or_default();
    Ok(pick_ne_song(songs, query))
}

/// The ne_search selection rules: a similar title, duration ±6 s, no "extra" versions (Remix and so on),
/// without a duration the artist must match exactly; points — artist 10, exact title 5, duration closeness up to 5.
pub fn pick_ne_song(songs: Vec<Value>, query: &LyricsQuery) -> Option<Value> {
    let (title, artist, duration) = (query.title.as_str(), query.artist.as_str(), query.duration);
    let primary = primary_artist(artist);
    let title_clean = clean_title(title);
    let title_tags = version_tags(title);
    let mut best = None;
    let mut best_score = -1.0;
    for s in songs {
        let name = text_of(get(&s, "name")).to_owned();
        let artists: Vec<&str> = get(&s, "ar").as_array().map(|a| a.iter().map(|a| text_of(get(a, "name"))).collect()).unwrap_or_default();
        let dt = get(&s, "dt").as_f64().unwrap_or(0.0) / 1000.0;
        if !similar(&clean_title(&name), &title_clean) {
            continue;
        }
        let a_ok = artists.iter().any(|a| similar(a, &primary)) || similar(&artists.join(" "), artist);
        let diff = if duration > 0.0 { (dt - duration).abs() } else { 0.0 };
        if duration > 0.0 && diff > 6.0 {
            continue;
        }
        // "Song (Remix)" when "Song" is playing: different timing
        if version_tags(&name).iter().any(|t| !title_tags.contains(t)) {
            continue;
        }
        let score = (if a_ok { 10.0 } else { 0.0 }) + (if norm(&name) == norm(title) { 5.0 } else { 0.0 }) + (5.0 - diff).max(0.0);
        if !a_ok && duration <= 0.0 {
            continue;
        }
        if score > best_score {
            best = Some(s);
            best_score = score;
        }
    }
    best
}

/// The NetEase lyrics request parameters in Python's key order
pub fn ne_lyric_json(song_id: &str) -> String {
    let mut s = format!("{{\"id\":{}", Value::String(song_id.to_owned()));
    s.push_str(",\"cp\":\"false\"");
    for key in ["lv", "kv", "tv", "rv", "yv", "ytv", "yrv"] {
        s.push_str(&format!(",\"{key}\":\"0\""));
    }
    s.push_str(",\"csrf_token\":\"\"}");
    ensure_ascii(&s)
}

/// The /eapi/song/lyric/v1 response (the yrc.lyric and lrc.lyric fields).
pub async fn ne_lyric(http: &reqwest::Client, song_id: i64) -> Result<Value, String> {
    ne_lyric_by_id(http, &song_id.to_string()).await
}

/// The same for a string id (str(song["id"]) in Python).
pub async fn ne_lyric_by_id(http: &reqwest::Client, song_id: &str) -> Result<Value, String> {
    let params = ne_eapi_params_text(NE_LYRIC_PATH, &ne_lyric_json(song_id));
    let url = "https://interface3.music.163.com/eapi/song/lyric/v1";
    Ok(http_json(|| {
        http.post(url)
            .form(&[("params", params.as_str())])
            .header(REFERER, NE_REFERER)
            .header("Cookie", "os=pc; appver=2.10.13")
            .timeout(timeout())
    })
    .await?)
}

/// json.dumps(ensure_ascii=True): all non-ASCII is \\uXXXX (outside the BMP — a surrogate pair)
fn ensure_ascii(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            let mut buf = [0u16; 2];
            for u in c.encode_utf16(&mut buf) {
                out.push_str(&format!("\\u{u:04x}"));
            }
        }
    }
    out
}

/// NetEase eapi encryption. The keys are in `params` traversal order (serde_json without preserve_order — alphabetical).
pub fn ne_eapi_params(path: &str, params: &Value) -> String {
    let text = ensure_ascii(&serde_json::to_string(params).unwrap_or_default());
    ne_eapi_params_text(path, &text)
}

/// eapi encryption over a finished JSON text (md5 takes that same text)
pub fn ne_eapi_params_text(path: &str, text: &str) -> String {
    let digest = hex::encode(Md5::digest(format!("nobody{path}use{text}md5forencrypt").as_bytes()));
    let data = format!("{path}-36cd479b6b5-{text}-36cd479b6b5-{digest}");
    let enc = ecb::Encryptor::<Aes128>::new(NE_KEY.into()).encrypt_padded_vec_mut::<Pkcs7>(data.as_bytes());
    hex::encode_upper(enc)
}

// ── Musixmatch ───────────────────────────────────────────────────────────────

/// The Musixmatch token (the user's or an anonymous one from the cache/token.get); None — blocked.
pub async fn mxm_token(http: &reqwest::Client, cache_dir: &Path, user_token: &str) -> Result<Option<String>, String> {
    if !user_token.is_empty() {
        return Ok(Some(user_token.to_owned()));
    }
    let path = cache_dir.join("mxm_token.json");
    if let Ok(raw) = tokio::fs::read_to_string(&path).await
        && let Ok(c) = serde_json::from_str::<Value>(&raw)
        && get(&c, "expires").as_f64().unwrap_or(0.0) > now_secs()
    {
        // None — blocked, retry later
        return Ok(get(&c, "token").as_str().map(str::to_owned));
    }
    let url = format!("{MXM_BASE}token.get?app_id={MXM_APP_ID}&format=json");
    let tok = match http_json(|| with_mxm_headers(http.get(&url)).timeout(Duration::from_secs(5))).await {
        Ok(d) => get(get(get(&d, "message"), "body"), "user_token")
            .as_str()
            .filter(|t| !t.is_empty() && t.chars().any(|c| c != '0'))
            .map(str::to_owned),
        Err(e) => {
            tracing::debug!("lyrics: mxm token error {e}");
            None
        }
    };
    let expires = now_secs() + if tok.is_some() { 6.0 * 3600.0 } else { 3600.0 };
    let body = json!({"token": tok, "expires": expires}).to_string();
    tokio::fs::write(&path, body).await.map_err(|e| e.to_string())?;
    Ok(tok)
}

fn with_mxm_headers(mut req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
    for (k, v) in MXM_HEADERS {
        req = req.header(k, v);
    }
    req
}

async fn mxm_call(http: &reqwest::Client, method: &str, token: &str, params: &[(&str, String)]) -> Result<Value, HttpError> {
    let url = format!("{MXM_BASE}{method}");
    let mut all: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, v.as_str())).collect();
    all.extend([("app_id", MXM_APP_ID), ("format", "json"), ("usertoken", token)]);
    let d = http_json(|| with_mxm_headers(http.get(&url).query(&all)).timeout(timeout())).await?;
    Ok(get(&d, "message").clone())
}

/// ("word", lines) / ("line", lines) / None.
pub async fn mxm_fetch(http: &reqwest::Client, query: &LyricsQuery, token: &str) -> Result<Option<(&'static str, Vec<Line>)>, String> {
    let duration = query.duration;
    let q_duration = if duration != 0.0 { (duration.trunc() as i64).to_string() } else { String::new() };
    let msg = mxm_call(
        http,
        "macro.subtitles.get",
        token,
        &[
            ("q_track", clean_title(&query.title)),
            ("q_artist", query.artist.clone()),
            ("q_duration", q_duration),
            ("namespace", "lyrics_richsynched".into()),
            ("subtitle_format", "mxm".into()),
            ("optional_calls", "track.richsync".into()),
        ],
    )
    .await?;
    let calls = get(get(&msg, "body"), "macro_calls");
    let track = get(get(get(get(calls, "matcher.track.get"), "message"), "body"), "track");
    if !truthy(track) {
        return Ok(None);
    }
    if truthy(get(track, "has_richsync")) {
        let rs = mxm_call(
            http,
            "track.richsync.get",
            token,
            &[
                ("commontrack_id", py_str(get(track, "commontrack_id"))),
                ("f_richsync_length", py_str(get(track, "track_length"))),
            ],
        )
        .await?;
        let body = get(get(get(&rs, "body"), "richsync"), "richsync_body");
        if let Some(body) = body.as_str().filter(|b| !b.is_empty()) {
            let lines = parse_richsync(body)?;
            if !lines.is_empty() {
                return Ok(Some(("word", lines)));
            }
        }
    }
    let sub = get(get(get(get(calls, "track.subtitles.get"), "message"), "body"), "subtitle_list");
    if let Some(first) = sub.as_array().and_then(|s| s.first()) {
        let body = get(get(first, "subtitle"), "subtitle_body");
        let body = body.as_str().ok_or("subtitle_body missing")?;
        let pairs = parse_subtitles(body)?;
        return Ok(Some(("line", build_from_timed_lines(&pairs, duration))));
    }
    Ok(None)
}

/// richsync_body: [{"ts", "te", "l": [{"c": text, "o": offset from ts}]}] → word-by-word lines.
/// A whitespace chunk is glued to the previous syllable (it is the one that ends the word).
pub fn parse_richsync(body: &str) -> Result<Vec<Line>, String> {
    let items: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let mut lines = Vec::new();
    for ln in items.as_array().ok_or("richsync_body is not a list")? {
        let ts = py_float(get(ln, "ts"), "ts")?;
        let te = py_float(get(ln, "te"), "te")?;
        let chunks = get(ln, "l").as_array().map(Vec::as_slice).unwrap_or_default();
        let mut sylls: Vec<(String, f64, f64)> = Vec::new();
        for (i, ch) in chunks.iter().enumerate() {
            let c = get(ch, "c").as_str().ok_or("richsync chunk without text")?;
            let s = ts + py_float(get(ch, "o"), "o")?;
            let e = match chunks.get(i + 1) {
                Some(next) => ts + py_float(get(next, "o"), "o")?,
                None => te,
            };
            if crate::text::py_strip(c).is_empty()
                && let Some(last) = sylls.last_mut()
            {
                last.0.push_str(c);
            } else if !c.is_empty() {
                sylls.push((c.to_owned(), s, e));
            }
        }
        let words = group_syllables(&sylls);
        if !words.is_empty() {
            lines.push(Line::words(round3(ts), round3(te), words, false));
        }
    }
    Ok(lines)
}

/// subtitle_body: [{"text", "time": {"total"}}] → (time, text) pairs
pub fn parse_subtitles(body: &str) -> Result<Vec<(f64, String)>, String> {
    let items: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    items
        .as_array()
        .ok_or("subtitle_body is not a list")?
        .iter()
        .map(|it| Ok((py_float(get(get(it, "time"), "total"), "time.total")?, text_of(get(it, "text")).to_owned())))
        .collect()
}

// ── LRCLIB ───────────────────────────────────────────────────────────────────

/// syncedLyrics from LRCLIB. The candidates are the exact get and a search by title with the artist, and if none of them
/// fits — all of the artist's tracks (LRCLIB's search matches whole words: it will not find
/// "Pagibat'"). Of the synced records with a similar title and duration, the markup most of them
/// agree on is taken — the same markup is often copied onto versions of a different length.
pub async fn lrclib_fetch(http: &reqwest::Client, query: &LyricsQuery) -> Result<Option<String>, String> {
    let duration = query.duration;
    let mut params: Vec<(&str, String)> = vec![("track_name", query.title.clone()), ("artist_name", query.artist.clone())];
    if !query.album.is_empty() {
        params.push(("album_name", query.album.clone()));
    }
    if duration != 0.0 {
        params.push(("duration", (duration.round_ties_even() as i64).to_string()));
    }
    let mut found = Vec::new();
    match lrclib(http, "https://lrclib.net/api/get", &params).await {
        Ok(d) => found.push(d),
        Err(HttpError::Status(404)) => {}
        Err(e) => return Err(e.into()),
    }
    let artist = primary_artist(&query.artist);
    let q = [("track_name", clean_title(&query.title)), ("artist_name", artist.clone())];
    extend_found(&mut found, lrclib(http, "https://lrclib.net/api/search", &q).await)?;
    if let Some(lrc) = pick_lrclib(&found, query) {
        return Ok(Some(lrc));
    }
    extend_found(&mut found, lrclib(http, "https://lrclib.net/api/search", &[("q", artist)]).await)?;
    Ok(pick_lrclib(&found, query))
}

fn extend_found(found: &mut Vec<Value>, res: Result<Value, HttpError>) -> Result<(), String> {
    match res {
        Ok(Value::Array(rows)) => found.extend(rows),
        Ok(_) => {}
        // there is already something to choose from — a failure of the extra search is not a problem
        Err(_) if !found.is_empty() => {}
        Err(e) => return Err(e.into()),
    }
    Ok(())
}

/// Similar titles: a match/substring after normalization or a difference in a pair of letters (≥ 80 % similarity).
fn title_like(a: &str, b: &str) -> bool {
    if similar(a, b) {
        return true;
    }
    let (a, b): (Vec<char>, Vec<char>) = (norm(a).chars().collect(), norm(b).chars().collect());
    let longest = a.len().max(b.len());
    longest >= 4 && levenshtein(&a, &b) as f64 <= longest as f64 * 0.2
}

fn levenshtein(a: &[char], b: &[char]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1; b.len() + 1];
        for (j, cb) in b.iter().enumerate() {
            cur[j + 1] = (prev[j] + usize::from(ca != cb)).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        prev = cur;
    }
    prev[b.len()]
}

struct LrcCandidate<'a> {
    lrc: &'a str,
    duration: f64,
    /// the times of the first lines with text — the markup's "handwriting"
    stamps: Vec<f64>,
}

impl LrcCandidate<'_> {
    fn same_sync(&self, other: &Self) -> bool {
        let n = self.stamps.len().min(other.stamps.len());
        n > 0 && self.stamps.iter().zip(&other.stamps).take(n).all(|(a, b)| (a - b).abs() <= 0.6)
    }
}

fn pick_lrclib(found: &[Value], query: &LyricsQuery) -> Option<String> {
    let title = clean_title(&query.title);
    let artist = primary_artist(&query.artist);
    let duration = query.duration;
    let mut ids = std::collections::HashSet::new();
    let mut all = Vec::new();
    for r in found {
        let Some(lrc) = get(r, "syncedLyrics").as_str().filter(|s| !s.is_empty()) else { continue };
        if let Some(id) = get(r, "id").as_i64()
            && !ids.insert(id)
        {
            continue;
        }
        if !title_like(&clean_title(text_of(get(r, "trackName"))), &title) || !similar(text_of(get(r, "artistName")), &artist) {
            continue;
        }
        let stamps = crate::lrc::lines_from_lrc(lrc).into_iter().filter(|(_, t)| !t.is_empty()).map(|(t, _)| t).take(3).collect();
        all.push(LrcCandidate { lrc, duration: get(r, "duration").as_f64().unwrap_or(0.0), stamps });
    }
    if duration <= 0.0 {
        return all.first().map(|c| c.lrc.to_owned());
    }
    let off = |c: &LrcCandidate| (c.duration - duration).abs();
    let near: Vec<&LrcCandidate> = all.iter().filter(|c| off(c) <= 3.0).collect();
    let pool = if near.is_empty() { all.iter().filter(|c| off(c) <= 5.0).collect() } else { near };
    pool.iter()
        .max_by(|a, b| {
            let score = |c: &LrcCandidate| {
                let support = pool.iter().filter(|o| o.same_sync(c)).count() as i64;
                let copies = all.iter().filter(|o| off(o) > 10.0 && o.same_sync(c)).count() as i64;
                2 * support - copies
            };
            score(a).cmp(&score(b)).then(off(b).total_cmp(&off(a)))
        })
        .map(|c| c.lrc.to_owned())
}

async fn lrclib(http: &reqwest::Client, url: &str, params: &[(&str, String)]) -> Result<Value, HttpError> {
    let q: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, v.as_str())).collect();
    http_json(|| http.get(url).query(&q).header(LRCLIB_HEADER.0, LRCLIB_HEADER.1).timeout(timeout())).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(title: &str, artist: &str, duration: f64) -> LyricsQuery {
        LyricsQuery { title: title.into(), artist: artist.into(), duration, ..Default::default() }
    }

    fn lrc_row(id: i64, title: &str, artist: &str, duration: f64, first: &str) -> Value {
        json!({"id": id, "trackName": title, "artistName": artist, "duration": duration,
               "syncedLyrics": format!("[{first}] a
[00:30.00] b
[00:40.00] c")})
    }

    #[test]
    fn lrclib_prefers_agreeing_sync_over_copied_one() {
        // markup with 00:21 was copied onto versions of any length, independent syncs converge on 00:02
        let rows = vec![
            lrc_row(1, "No Coke", "Dr. Alban", 224.0, "00:21.16"),
            lrc_row(2, "No Coke", "Dr. Alban", 222.0, "00:21.16"),
            lrc_row(3, "No coke", "Dr. Alban", 245.0, "00:21.16"),
            lrc_row(4, "No Coke", "Dr. Alban", 402.0, "00:21.16"),
            lrc_row(5, "No Coke", "Dr. Alban", 256.0, "00:21.16"),
            lrc_row(6, "No Coke", "Dr. Alban", 224.0, "00:02.41"),
            lrc_row(7, "No Coke 7 Mix", "Dr Alban", 224.5, "00:02.77"),
            lrc_row(8, "No Coke", "Dr. Alban", 225.0, "00:02.49"),
        ];
        let lrc = pick_lrclib(&rows, &q("No Coke", "Dr. Alban", 224.0)).unwrap();
        assert!(lrc.starts_with("[00:02"), "{lrc}");
    }

    #[test]
    fn lrclib_title_tolerates_spelling() {
        let rows = vec![
            lrc_row(1, "Slip under the skin", "Mr Small", 134.0, "00:10.00"),
            lrc_row(2, "Will be dieing young", "Mr Small", 134.0, "00:17.00"),
        ];
        let lrc = pick_lrclib(&rows, &q("Will be dying young", "Mr Small", 135.0)).unwrap();
        assert!(lrc.starts_with("[00:17"));
        assert!(!title_like("Slip under the skin", "Will be dying young"));
        assert!(pick_lrclib(&rows, &q("Will be dying young", "Other", 135.0)).is_none());
    }

    #[tokio::test]
    #[ignore = "network: real LRCLIB"]
    async fn lrclib_live() {
        let http = reqwest::Client::new();
        let yest = lrclib_fetch(&http, &q("Yesterday", "The Beatles", 125.0)).await.unwrap().unwrap();
        assert!(yest.contains("[00:"), "{}", &yest[..80.min(yest.len())]);
        let coke = lrclib_fetch(&http, &q("No Coke", "Dr. Alban", 224.0)).await.unwrap().unwrap();
        assert!(coke.trim_start().starts_with("[00:02"), "{}", &coke[..80.min(coke.len())]);
    }

    #[test]
    fn ne_lyric_json_keeps_python_order() {
        assert_eq!(
            ne_lyric_json("123"),
            r#"{"id":"123","cp":"false","lv":"0","kv":"0","tv":"0","rv":"0","yv":"0","ytv":"0","yrv":"0","csrf_token":""}"#
        );
        assert_eq!(ensure_ascii("é😀"), "\\u00e9\\ud83d\\ude00");
    }

    #[test]
    fn ne_song_is_picked_by_rules() {
        let songs = json!([
            {"id": 1, "name": "Creep (Acoustic)", "ar": [{"name": "Radiohead"}], "dt": 238000},
            {"id": 2, "name": "Creep", "ar": [{"name": "Someone"}], "dt": 239000},
            {"id": 3, "name": "Creep", "ar": [{"name": "Radiohead"}], "dt": 236000},
            {"id": 4, "name": "Creep", "ar": [{"name": "Radiohead"}], "dt": 300000},
            {"id": 5, "name": "Other", "ar": [{"name": "Radiohead"}], "dt": 238000}
        ]);
        let songs = songs.as_array().unwrap().clone();
        let best = pick_ne_song(songs.clone(), &q("Creep", "Radiohead", 238.0)).unwrap();
        assert_eq!(best["id"], 3);
        // without a duration another artist does not fit
        let best = pick_ne_song(songs[1..2].to_vec(), &q("Creep", "Radiohead", 0.0));
        assert!(best.is_none());
        // a version that is in the playing track's title is not an extra one
        let best = pick_ne_song(songs, &q("Creep (Acoustic)", "Radiohead", 238.0)).unwrap();
        assert_eq!(best["id"], 1);
    }

    #[test]
    fn richsync_chunks_become_words() {
        let body = r#"[{"ts": 1.0, "te": 3.0, "l": [{"c": "Hel", "o": 0}, {"c": "lo", "o": 0.5}, {"c": " ", "o": 0.9}, {"c": "world", "o": 1.0}]}]"#;
        let lines = parse_richsync(body).unwrap();
        let Line::Words { t, e, w, i, .. } = &lines[0] else { panic!() };
        assert_eq!((*t, *e, *i), (1.0, 3.0, false));
        assert_eq!(w.iter().map(|w| w.x.as_str()).collect::<Vec<_>>(), vec!["Hello ", "world"]);
        assert_eq!(w[0].p, Some(vec![(3, 1.0, 1.5), (2, 1.5, 1.9)]));
        assert_eq!((w[1].s, w[1].e), (2.0, 3.0));
    }

    #[test]
    fn subtitles_are_pairs() {
        let pairs = parse_subtitles(r#"[{"text": "a", "time": {"total": 1.5}}, {"time": {"total": "2"}}]"#).unwrap();
        assert_eq!(pairs, vec![(1.5, "a".to_owned()), (2.0, String::new())]);
    }

    #[test]
    fn python_truthiness() {
        assert!(!truthy(&json!({})));
        assert!(truthy(&json!(1)));
        assert!(!truthy(&json!(0)));
        assert_eq!(py_str(&json!(null)), "None");
        assert_eq!(py_str(&json!(12)), "12");
    }
}
