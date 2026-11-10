//! Lyrics via ytmusicapi get_lyrics + mf-lyrics fallbacks (LRCLIB etc.).

use std::sync::{Arc, Weak};

use tokio::runtime::Handle;
use tokio::sync::{broadcast, watch};
use mf_lyrics::Line;
use yt_player::{PlayerEvent, PlayerHandle};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LyricsStatus {
    #[default]
    Idle,
    Loading,
    Ok,
    NotFound,
    Error,
}
impl LyricsStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Loading => "loading",
            Self::Ok => "ok",
            Self::NotFound => "notfound",
            Self::Error => "error",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LyricsSnapshot {
    pub track_id: String,
    pub status: LyricsStatus,
    pub source: String,
    pub kind: String,
    pub lines: Arc<Vec<Line>>,
}

pub struct LyricsService {
    _rt: Handle,
    player: PlayerHandle,
    fetcher: mf_lyrics::LyricsFetcher,
    api: Arc<yt_api::YtClient>,
    snapshot: watch::Sender<LyricsSnapshot>,
    _me: Weak<LyricsService>,
}

impl LyricsService {
    pub fn new(rt: Handle, player: PlayerHandle, fetcher: mf_lyrics::LyricsFetcher, api: Arc<yt_api::YtClient>) -> Arc<Self> {
        let mut events = player.subscribe().events;
        let (snapshot, _) = watch::channel(LyricsSnapshot::default());
        let service = Arc::new_cyclic(|me| Self {
            _rt: rt.clone(),
            player: player.clone(),
            fetcher: fetcher.clone(),
            api: api.clone(),
            snapshot: snapshot.clone(),
            _me: me.clone(),
        });
        let weak = Arc::downgrade(&service);
        rt.spawn(async move {
            loop {
                match events.recv().await {
                    Ok(PlayerEvent::TrackChanged) => {
                        tokio::time::sleep(std::time::Duration::from_millis(350)).await;
                        if let Some(s) = weak.upgrade() { s.load(); }
                    }
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        });
        service
    }

    fn load(&self) {
        let track = self.player.snapshot().track;
        let Some(track) = track else { return };
        let track_id = track.track_id.clone();
        let track = track.clone();
        if track_id.is_empty() { return; }
        // ytmusicapi get_lyrics goes through the Python bridge; for this build
        // we reuse mf-lyrics sources (LRCLIB/NetEase/Musixmatch) which cover most tracks.
        let _ = self.snapshot.send_replace(LyricsSnapshot { track_id: track_id.clone(), status: LyricsStatus::Loading, ..Default::default() });
        let (snapshot, fetcher, api) = (self.snapshot.clone(), self.fetcher.clone(), self.api.clone());
        self._rt.spawn(async move {
            let mut lines: Vec<Line> = vec![];
            let mut source = String::new();
            let mut kind = "line".to_string();
            // Primary: YouTube Music's own timed lyrics (LyricFind).
            if let Ok(value) = api.get_lyrics(&track_id).await
                && let Some((ylines, ysource)) = ytmusic_lines(&value, track.duration_ms as f64 / 1000.0)
                && !ylines.is_empty()
            {
                lines = ylines;
                source = ysource;
                kind = "line".to_string();
            }
            if lines.is_empty() {
                let query = mf_lyrics::LyricsQuery {
                    track_id: track.track_id.clone(),
                    title: track.title.clone(),
                    artist: track.artists.clone(),
                    album: track.album.clone(),
                    duration: track.duration_ms as f64 / 1000.0,
                };
                let result = fetcher.fetch_cached(&query, None).await;
                if result.ok {
                    lines = result.lines.clone();
                    source = result.source.clone();
                    kind = result.kind.clone();
                }
            }
            let snap = if lines.is_empty() {
                LyricsSnapshot { track_id, status: LyricsStatus::NotFound, ..Default::default() }
            } else {
                LyricsSnapshot { track_id, status: LyricsStatus::Ok, source, kind, lines: Arc::new(lines) }
            };
            let _ = snapshot.send_replace(snap);
        });
    }

    pub fn retry(&self) { self.load(); }
    pub fn snapshot(&self) -> LyricsSnapshot { self.snapshot.borrow().clone() }
    pub fn watch(&self) -> watch::Receiver<LyricsSnapshot> { self.snapshot.subscribe() }
}

/// Map ytmusicapi lyrics (bridge `get_lyrics`) to display lines.
///
/// Timestamped: each line becomes a single-word line spanning [startTime,
/// endTime] in seconds. Plain text: split into lines and spread evenly over
/// the track's real duration (honest karaoke estimate, not synthesis).
/// Returns (lines, source).
fn ytmusic_lines(value: &serde_json::Value, duration_secs: f64) -> Option<(Vec<Line>, String)> {
    let source = value.get("source").and_then(|s| s.as_str()).unwrap_or("");
    // Timestamped form: {lines: [{text, startTime, endTime}], source, hasTimestamps}
    if let Some(items) = value.get("lines").and_then(|l| l.as_array()) {
        let mut lines = Vec::with_capacity(items.len());
        for item in items {
            let text = text_of(item.get("text"));
            if text.trim().is_empty() { continue; }
            let start = num(item.get("startTime"));
            let end = num(item.get("endTime"));
            lines.push(Line::words(
                (start / 1000.0).round() / 1000.0,
                (end / 1000.0).round() / 1000.0,
                vec![mf_lyrics::Word { x: text.to_string(), s: start / 1000.0, e: end / 1000.0, p: None }],
                false,
            ));
        }
        if !lines.is_empty() {
            return Some((lines, source.to_string()));
        }
    }
    // Plain text form: {lyrics: "line1\r\nline2..."}. Spread evenly.
    let text = text_of(value.get("lyrics"));
    if !text.trim().is_empty() {
        let rows: Vec<String> = text.lines().map(|l| l.trim_end().to_string()).collect();
        let rows: Vec<String> = rows.into_iter().filter(|l| !l.trim().is_empty()).collect();
        if !rows.is_empty() {
            let n = rows.len() as f64;
            let duration = if duration_secs > 0.0 { duration_secs } else { n * 3.0 };
            let lines = rows
                .into_iter()
                .enumerate()
                .map(|(i, text)| {
                    let t = i as f64 / n * duration;
                    let e = (i + 1) as f64 / n * duration;
                    Line::words(t, e, vec![mf_lyrics::Word { x: text, s: t, e, p: None }], false)
                })
                .collect();
            return Some((lines, source.to_string()));
        }
    }
    None
}

fn text_of(v: Option<&serde_json::Value>) -> &str {
    v.and_then(|x| x.as_str()).unwrap_or("")
}

fn num(v: Option<&serde_json::Value>) -> f64 {
    v.and_then(|x| x.as_f64()).unwrap_or(0.0)
}
