//! Local audio output through rodio (cpal), fed directly from InnerTube.
//!
//! YouTube Music serves audio as `audio/mp4` (AAC) and `audio/webm` (Opus).
//! Symphonia, and therefore rodio, decodes the AAC stream; the picker in
//! `innertube.rs` prefers it.
//!
//! Nothing is downloaded to disk and nothing is downloaded up front: a
//! background thread pulls byte windows from the CDN URL with HTTP `Range`
//! requests, decodes them, and pushes PCM into a small bounded channel. The
//! rodio source only pops from that channel, so the real-time audio callback
//! never blocks on the network.

use std::io::{self, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use rodio::{ChannelCount, Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Sample, SampleRate, Source};

/// Window size for Range fetches. 256 KiB stays under Google Video's ~1 MiB
/// per-token cap while amortizing HTTP overhead.
const WINDOW: u64 = 256 * 1024;

/// PCM batches queued ahead of playback. 16 batches of 8192 frames is roughly
/// three seconds at 44.1 kHz stereo: enough to ride out jitter, small enough
/// that a seek doesn't leave stale audio behind for long.
const BATCH_FRAMES: usize = 8192;
const BATCHES: usize = 16;

/// Seekable reader that fetches byte windows from a googlevideo CDN URL on
/// demand (HTTP Range). rodio's `Decoder` requires `Read + Seek`, and
/// symphonia may seek to the `moov` atom; this makes that a real stream —
/// only the bytes the decoder asks for are fetched.
struct StreamReader {
    url: String,
    client: reqwest::blocking::Client,
    len: Option<u64>,
    pos: u64,
    buf: Vec<u8>,
    buf_start: u64,
}

impl StreamReader {
    fn new(url: String) -> io::Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(io::Error::other)?;
        Ok(Self { url, client, len: None, pos: 0, buf: Vec::new(), buf_start: 0 })
    }

    fn ensure_len(&mut self) -> io::Result<u64> {
        if let Some(len) = self.len {
            return Ok(len);
        }
        let res = self
            .client
            .get(&self.url)
            .header("Range", "bytes=0-0")
            .header("User-Agent", "Mozilla/5.0")
            .send()
            .map_err(io::Error::other)?;
        if !res.status().is_success() && res.status() != reqwest::StatusCode::PARTIAL_CONTENT {
            return Err(io::Error::other(format!("CDN HTTP {}", res.status())));
        }
        let total = res
            .headers()
            .get("content-range")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.rsplit('/').next())
            .and_then(|v| v.trim().parse::<u64>().ok())
            .or_else(|| res.content_length());
        let len = total.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "CDN gave no length"))?;
        self.len = Some(len);
        Ok(len)
    }

    fn fill(&mut self, at: u64) -> io::Result<()> {
        if at >= self.buf_start && at < self.buf_start + self.buf.len() as u64 {
            return Ok(());
        }
        let end = at.saturating_add(WINDOW - 1);
        let res = self
            .client
            .get(&self.url)
            .header("Range", format!("bytes={at}-{end}"))
            .header("User-Agent", "Mozilla/5.0")
            .send()
            .map_err(io::Error::other)?;
        let status = res.status();
        if !status.is_success() && status != reqwest::StatusCode::PARTIAL_CONTENT {
            return Err(io::Error::other(format!("CDN HTTP {status}")));
        }
        let body = res.bytes().map_err(io::Error::other)?;
        // A server that ignores Range answers 200 with the whole file; that
        // body starts at byte 0, not at `at`.
        self.buf_start = if status == reqwest::StatusCode::OK { 0 } else { at };
        self.buf = body.to_vec();
        Ok(())
    }
}

impl Read for StreamReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        let len = self.ensure_len()?;
        if self.pos >= len {
            return Ok(0);
        }
        self.fill(self.pos)?;
        if self.pos < self.buf_start {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "CDN returned the wrong window"));
        }
        let avail = (self.buf_start + self.buf.len() as u64 - self.pos) as usize;
        if avail == 0 {
            return Ok(0);
        }
        let n = avail.min(out.len());
        let off = (self.pos - self.buf_start) as usize;
        out[..n].copy_from_slice(&self.buf[off..off + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for StreamReader {
    fn seek(&mut self, style: SeekFrom) -> io::Result<u64> {
        let len = self.ensure_len()?;
        let target: i128 = match style {
            SeekFrom::Start(o) => o as i128,
            SeekFrom::End(o) => len as i128 + o as i128,
            SeekFrom::Current(o) => self.pos as i128 + o as i128,
        };
        if target < 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "seek before start"));
        }
        self.pos = target as u64;
        Ok(self.pos)
    }
}

/// rodio source backed by the decoder thread's PCM channel. When the buffer
/// runs dry it yields silence instead of blocking the audio callback.
struct StreamSource {
    rx: mpsc::Receiver<Vec<Sample>>,
    batch: Vec<Sample>,
    at: usize,
    channels: ChannelCount,
    sample_rate: SampleRate,
}

impl StreamSource {
    fn new(rx: mpsc::Receiver<Vec<Sample>>, channels: ChannelCount, sample_rate: SampleRate) -> Self {
        Self { rx, batch: Vec::new(), at: 0, channels, sample_rate }
    }
}

impl Iterator for StreamSource {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        loop {
            if self.at < self.batch.len() {
                let sample = self.batch[self.at];
                self.at += 1;
                return Some(sample);
            }
            match self.rx.try_recv() {
                Ok(batch) => {
                    self.batch = batch;
                    self.at = 0;
                }
                Err(mpsc::TryRecvError::Empty) => return Some(0.0),
                Err(mpsc::TryRecvError::Disconnected) => return None,
            }
        }
    }
}

impl Source for StreamSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        self.channels
    }

    fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

/// Prefix of the error reported when the output can't be opened (`{OPEN_FAILED}: detail`).
pub const OPEN_FAILED: &str = "Couldn't open the audio output device";

/// What the audio thread decodes: a CDN stream fetched on demand, or a fully
/// downloaded file held in memory.
#[derive(Clone)]
enum AudioSource {
    /// Streamed byte windows from a googlevideo URL (Range requests).
    Url(String),
    /// A complete file already downloaded (yt-dlp). No network at play time.
    Bytes(Arc<Vec<u8>>),
}

enum Cmd {
    Load(AudioSource),
    Pause(bool),
    Stop,
    Seek(u64),
    Volume(f64),
}

struct Shared {
    position_ms: AtomicI64,
    paused: AtomicBool,
    ended: AtomicBool,
    has_content: AtomicBool,
    error: Mutex<Option<String>>,
    /// RMS of the most recently decoded PCM (for the level meters in QML),
    /// stored as an f32 bit pattern. An Arc so the decode thread and the
    /// audio thread share it.
    level: Arc<AtomicU32>,
}

/// Handle to the audio thread. All methods are non-blocking.
pub struct Audio {
    tx: mpsc::Sender<Cmd>,
    shared: Arc<Shared>,
}

impl Audio {
    pub fn new(volume: f64) -> Self {
        let (tx, rx) = mpsc::channel();
        let shared = Arc::new(Shared {
            position_ms: AtomicI64::new(0),
            paused: AtomicBool::new(false),
            ended: AtomicBool::new(false),
            has_content: AtomicBool::new(false),
            error: Mutex::new(None),
            level: Arc::new(AtomicU32::new(0)),
        });
        let thread_shared = shared.clone();
        let spawn = thread::Builder::new()
            .name("yt-audio".into())
            .spawn(move || run(rx, thread_shared, volume));
        if let Err(e) = spawn {
            set_error(&shared, format!("Couldn't start the audio thread: {e}"));
        }
        Self { tx, shared }
    }

    /// Replace the current stream with `url` (InnerTube/CDN), starting at 0.
    pub fn load(&self, url: String) {
        let _ = self.tx.send(Cmd::Load(AudioSource::Url(url)));
    }

    /// Play a fully downloaded file from memory (yt-dlp), starting at 0.
    pub fn load_bytes(&self, bytes: Vec<u8>) {
        let _ = self.tx.send(Cmd::Load(AudioSource::Bytes(Arc::new(bytes))));
    }

    pub fn set_paused(&self, paused: bool) {
        self.shared.paused.store(paused, Ordering::SeqCst);
        let _ = self.tx.send(Cmd::Pause(paused));
    }

    pub fn is_paused(&self) -> bool {
        self.shared.paused.load(Ordering::SeqCst)
    }

    pub fn stop(&self) {
        self.shared.position_ms.store(0, Ordering::SeqCst);
        self.shared.ended.store(false, Ordering::SeqCst);
        self.shared.has_content.store(false, Ordering::SeqCst);
        let _ = self.tx.send(Cmd::Stop);
    }

    pub fn seek(&self, ms: i64) {
        let _ = self.tx.send(Cmd::Seek(ms.max(0) as u64));
    }

    pub fn set_volume(&self, volume: f64) {
        let _ = self.tx.send(Cmd::Volume(volume.clamp(0.0, 1.0)));
    }

    pub fn position_ms(&self) -> i64 {
        self.shared.position_ms.load(Ordering::SeqCst)
    }

    /// Current decoded PCM level (0..1), for the QML level meters.
    pub fn level(&self) -> f32 {
        f32::from_bits(self.shared.level.load(Ordering::SeqCst))
    }

    /// True once the stream has been played to its end. Consumes the flag.
    pub fn take_ended(&self) -> bool {
        self.shared.ended.swap(false, Ordering::SeqCst)
    }

    /// The next output error, if any. Consumes it.
    pub fn take_error(&self) -> Option<String> {
        self.shared.error.lock().ok()?.take()
    }
}

fn set_error(shared: &Shared, message: String) {
    if let Ok(mut slot) = shared.error.lock() {
        *slot = Some(message);
    }
}

fn has_error(shared: &Shared) -> bool {
    shared.error.lock().map(|slot| slot.is_some()).unwrap_or(false)
}

struct Sink {
    player: Option<Player>,
    device: Option<MixerDeviceSink>,
    volume: f32,
    source: Option<AudioSource>,
    base_ms: i64,
}

impl Sink {
    fn new(volume: f64) -> Self {
        Self { player: None, device: None, volume: volume as f32, source: None, base_ms: 0 }
    }

    /// Open the device once; report a failure through the shared slot.
    fn ensure_device(&mut self, shared: &Shared) -> bool {
        if self.device.is_some() {
            return true;
        }
        match DeviceSinkBuilder::open_default_sink() {
            Ok(mut opened) => {
                // The sink lives for the whole process; the drop notice only
                // fires on shutdown and would land on stderr for no reason.
                opened.log_on_drop(false);
                self.device = Some(opened);
                true
            }
            Err(e) => {
                set_error(shared, format!("{OPEN_FAILED}: {e}"));
                false
            }
        }
    }

    /// Start `source` at `start_ms`, replacing whatever is playing.
    fn start(&mut self, source: AudioSource, start_ms: i64, shared: &Shared) {
        match source.clone() {
            AudioSource::Url(url) => {
                self.start_with(
                    move || {
                        let reader = StreamReader::new(url.clone())
                            .map_err(|e| io::Error::other(format!("Audio stream failed: {e}")))?;
                        Decoder::new(reader)
                            .map_err(|e| io::Error::other(format!("Audio decode failed: {e}")))
                    },
                    start_ms, shared,
                )
            }
            AudioSource::Bytes(bytes) => {
                self.start_with(
                    move || Decoder::new(io::Cursor::new((*bytes).clone()))
                        .map_err(|e| io::Error::other(format!("Audio decode failed: {e}"))),
                    start_ms, shared,
                )
            }
        }
        self.source = Some(source);
    }

    /// Open the decoder (again on a failed seek), then hand PCM to rodio.
    /// `rebuild` is called for the first open and again if the seek fails.
    fn start_with<R: Read + Seek + Send + 'static>(
        &mut self,
        rebuild: impl Fn() -> io::Result<Decoder<R>>,
        start_ms: i64,
        shared: &Shared,
    ) {
        // Dropping the player stops the previous stream and drops its PCM
        // receiver, which ends the previous decoder thread.
        self.player = None;
        shared.ended.store(false, Ordering::SeqCst);
        shared.position_ms.store(0, Ordering::SeqCst);
        shared.has_content.store(false, Ordering::SeqCst);
        if !self.ensure_device(shared) {
            return;
        }
        let Some(mixer) = self.device.as_ref().map(|d| d.mixer()) else {
            return;
        };

        // The decoder header lives here, on the control thread, so the audio
        // callback never touches the network.
        let mut decoder = match rebuild() {
            Ok(decoder) => decoder,
            Err(e) => {
                set_error(shared, e.to_string());
                return;
            }
        };
        let channels = decoder.channels();
        let sample_rate = decoder.sample_rate();

        let mut base_ms = 0;
        if start_ms > 0 {
            match decoder.try_seek(Duration::from_millis(start_ms as u64)) {
                Ok(()) => base_ms = start_ms,
                Err(_) => {
                    // Not seekable: rebuild the decoder and restart from the
                    // beginning rather than pretending we moved.
                    match rebuild() {
                        Ok(fresh) => decoder = fresh,
                        Err(e) => {
                            set_error(shared, e.to_string());
                            return;
                        }
                    }
                }
            }
        }

        let (tx, rx) = mpsc::sync_channel::<Vec<Sample>>(BATCHES);
        let source = StreamSource::new(rx, channels, sample_rate);
        let player = Player::connect_new(mixer);
        player.set_volume(self.volume);
        player.append(source);
        if shared.paused.load(Ordering::SeqCst) {
            player.pause();
        }

        self.player = Some(player);
        self.base_ms = base_ms;
        shared.position_ms.store(base_ms, Ordering::SeqCst);
        shared.has_content.store(true, Ordering::SeqCst);
        let level = shared.level.clone();

        let spawned = thread::Builder::new()
            .name("yt-decode".into())
            .spawn(move || decode_loop(decoder, tx, level));
        if let Err(e) = spawned {
            set_error(shared, format!("Couldn't start the audio decoder: {e}"));
            self.player = None;
            shared.has_content.store(false, Ordering::SeqCst);
        }
    }

    fn seek(&mut self, ms: u64, shared: &Shared) {
        let Some(source) = self.source.clone() else { return };
        self.start(source, ms as i64, shared);
    }

    fn stop(&mut self, shared: &Shared) {
        self.player = None;
        self.source = None;
        self.base_ms = 0;
        shared.position_ms.store(0, Ordering::SeqCst);
        shared.ended.store(false, Ordering::SeqCst);
        shared.has_content.store(false, Ordering::SeqCst);
        shared.level.store(0, Ordering::SeqCst);
    }

    fn monitor(&self, shared: &Shared) {
        let Some(player) = &self.player else { return };
        let pos = self.base_ms + player.get_pos().as_millis() as i64;
        shared.position_ms.store(pos, Ordering::SeqCst);
        let paused = player.is_paused();
        // Paused or stopped: the meter must fall to silence (the decode loop
        // stops updating it while the rodio source is paused).
        if paused {
            shared.level.store(0, Ordering::SeqCst);
        }
        let empty = player.empty();
        if empty
            && !paused
            && shared.has_content.load(Ordering::SeqCst)
            && !has_error(shared)
        {
            shared.ended.store(true, Ordering::SeqCst);
        }
    }
}

/// Pull decoded PCM out of the decoder and into the bounded channel.
/// `send` blocks when the buffer is full, which applies backpressure all the
/// way to the audio source: memory stays bounded and only a few seconds of
/// audio are ever decoded ahead of the listener.
fn decode_loop<R: Read + Seek + Send + 'static>(mut decoder: Decoder<R>, tx: mpsc::SyncSender<Vec<Sample>>, level: Arc<AtomicU32>) {
    let channels = decoder.channels().get() as usize;
    let samples = BATCH_FRAMES.max(1) * channels;
    loop {
        let batch: Vec<Sample> = decoder.by_ref().take(samples).collect();
        if batch.is_empty() {
            break;
        }
        // RMS energy of this batch, for the QML level meters.
        let mean_square = batch.iter().map(|s| s * s).sum::<f32>() / batch.len() as f32;
        let rms = (mean_square.sqrt() * 2.0).clamp(0.0, 1.0);
        level.store(rms.to_bits(), Ordering::SeqCst);
        if tx.send(batch).is_err() {
            break;
        }
    }
}

fn run(rx: mpsc::Receiver<Cmd>, shared: Arc<Shared>, initial_volume: f64) {
    let mut sink = Sink::new(initial_volume);

    loop {
        match rx.recv_timeout(Duration::from_millis(40)) {
            Ok(Cmd::Load(source)) => sink.start(source, 0, &shared),
            Ok(Cmd::Pause(paused)) => {
                if let Some(p) = &sink.player {
                    if paused {
                        p.pause();
                    } else {
                        p.play();
                    }
                }
            }
            Ok(Cmd::Stop) => sink.stop(&shared),
            Ok(Cmd::Seek(ms)) => sink.seek(ms, &shared),
            Ok(Cmd::Volume(v)) => {
                sink.volume = v as f32;
                if let Some(p) = &sink.player {
                    p.set_volume(sink.volume);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        sink.monitor(&shared);
    }
}
