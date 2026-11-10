//! Playback speed and crossfade: live settings that the audio output reads on every packet.
//! Changed without restarting the engine; the track position in the UI accounts for the speed too.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

pub use super::stretch::SPEED_RANGE;

/// The longest crossfade Settings offers.
pub const CROSSFADE_MAX: Duration = Duration::from_secs(12);

static SPEED: AtomicU32 = AtomicU32::new(1.0f32.to_bits());
static CROSSFADE_MS: AtomicU32 = AtomicU32::new(0);
/// librespot reads it at each track change and while a DJ line plays (`PlayerConfig::narration`).
static DJ_VOICE: LazyLock<Arc<AtomicBool>> = LazyLock::new(|| Arc::new(AtomicBool::new(true)));

/// How fast the music plays, 1.0 being as recorded.
pub fn speed() -> f32 {
    f32::from_bits(SPEED.load(Ordering::Relaxed))
}

pub fn set_speed(speed: f32) {
    let speed = if speed.is_finite() { speed.clamp(*SPEED_RANGE.start(), *SPEED_RANGE.end()) } else { 1.0 };
    SPEED.store(speed.to_bits(), Ordering::Relaxed);
}

/// How long one track fades into the next; zero is off.
pub fn crossfade() -> Duration {
    Duration::from_millis(u64::from(CROSSFADE_MS.load(Ordering::Relaxed)))
}

pub fn set_crossfade(length: Duration) {
    let ms = length.min(CROSSFADE_MAX).as_millis() as u32;
    CROSSFADE_MS.store(ms, Ordering::Relaxed);
}

/// Whether the DJ says its lines between tracks. The same switch goes to every engine.
pub fn dj_voice() -> Arc<AtomicBool> {
    Arc::clone(&DJ_VOICE)
}

pub fn set_dj_voice(on: bool) {
    DJ_VOICE.store(on, Ordering::Relaxed);
}

/// Track time that passes in `elapsed` of wall time at the current speed.
pub fn track_elapsed_ms(elapsed: Duration) -> u32 {
    (elapsed.as_secs_f64() * f64::from(speed()) * 1_000.0) as u32
}
