//! Local audio output through rodio (cpal).
//!
//! YouTube Music serves audio as `audio/mp4` (AAC) and `audio/webm` (Opus).
//! Symphonia, and therefore rodio, decodes the AAC stream; the picker in
//! `stream.rs` prefers it. A dedicated thread owns the device sink because the
//! cpal stream is not `Send` on every platform, and because opening the device
//! must never block the player actor.

use std::io::Cursor;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player};

/// Prefix of the error reported when the output can't be opened (`{OPEN_FAILED}: detail`).
pub const OPEN_FAILED: &str = "Couldn't open the audio output device";

enum Cmd {
    Load(Vec<u8>),
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

    /// Replace the current stream with `bytes`. Decoding happens on the audio
    /// thread; a decode failure surfaces through `take_error`.
    pub fn load(&self, bytes: Vec<u8>) {
        let _ = self.tx.send(Cmd::Load(bytes));
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

fn run(rx: mpsc::Receiver<Cmd>, shared: Arc<Shared>, initial_volume: f64) {
    let mut device: Option<MixerDeviceSink> = None;
    let mut player: Option<Player> = None;
    let mut volume = initial_volume as f32;

    loop {
        match rx.recv_timeout(Duration::from_millis(40)) {
            Ok(Cmd::Load(bytes)) => {
                shared.ended.store(false, Ordering::SeqCst);
                shared.position_ms.store(0, Ordering::SeqCst);
                shared.has_content.store(false, Ordering::SeqCst);

                if device.is_none() {
                    match DeviceSinkBuilder::open_default_sink() {
                        Ok(opened) => device = Some(opened),
                        Err(e) => {
                            set_error(&shared, format!("{OPEN_FAILED}: {e}"));
                            player = None;
                            continue;
                        }
                    }
                }

                let mixer = device.as_ref().map(|d| d.mixer());
                let Some(mixer) = mixer else { continue };
                match Decoder::new(Cursor::new(bytes)) {
                    Ok(source) => {
                        let next = Player::connect_new(mixer);
                        next.set_volume(volume);
                        next.append(source);
                        if shared.paused.load(Ordering::SeqCst) {
                            next.pause();
                        }
                        // Dropping the previous player stops the old stream.
                        player = Some(next);
                        shared.has_content.store(true, Ordering::SeqCst);
                    }
                    Err(e) => {
                        player = None;
                        set_error(&shared, format!("Audio decode failed: {e}"));
                    }
                }
            }
            Ok(Cmd::Pause(paused)) => {
                if let Some(p) = &player {
                    if paused {
                        p.pause();
                    } else {
                        p.play();
                    }
                }
            }
            Ok(Cmd::Stop) => {
                player = None;
                shared.position_ms.store(0, Ordering::SeqCst);
                shared.has_content.store(false, Ordering::SeqCst);
            }
            Ok(Cmd::Seek(ms)) => {
                if let Some(p) = &player {
                    let _ = p.try_seek(Duration::from_millis(ms));
                    shared.position_ms.store(ms as i64, Ordering::SeqCst);
                }
            }
            Ok(Cmd::Volume(v)) => {
                volume = v as f32;
                if let Some(p) = &player {
                    p.set_volume(volume);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }

        if let Some(p) = &player {
            let pos = p.get_pos().as_millis() as i64;
            shared.position_ms.store(pos, Ordering::SeqCst);
            let paused = p.is_paused();
            let empty = p.empty();
            if empty && !paused && shared.has_content.load(Ordering::SeqCst) {
                shared.ended.store(true, Ordering::SeqCst);
            }
        }
    }
}
