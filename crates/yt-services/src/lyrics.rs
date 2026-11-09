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
    snapshot: watch::Sender<LyricsSnapshot>,
    _me: Weak<LyricsService>,
}

impl LyricsService {
    pub fn new(rt: Handle, player: PlayerHandle, fetcher: mf_lyrics::LyricsFetcher) -> Arc<Self> {
        let mut events = player.subscribe().events;
        let (snapshot, _) = watch::channel(LyricsSnapshot::default());
        let service = Arc::new_cyclic(|me| Self {
            _rt: rt.clone(),
            player: player.clone(),
            fetcher: fetcher.clone(),
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
        let _ = self.snapshot.send(LyricsSnapshot { track_id: track_id.clone(), status: LyricsStatus::Loading, ..Default::default() });
        let (snapshot, fetcher) = (self.snapshot.clone(), self.fetcher.clone());
        self._rt.spawn(async move {
            let mut lines: Vec<Line> = vec![];
            let mut source = String::new();
            let mut kind = "line".to_string();
            {
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
            let _ = snapshot.send(snap);
        });
    }

    pub fn retry(&self) { self.load(); }
    pub fn snapshot(&self) -> LyricsSnapshot { self.snapshot.borrow().clone() }
    pub fn watch(&self) -> watch::Receiver<LyricsSnapshot> { self.snapshot.subscribe() }
}
