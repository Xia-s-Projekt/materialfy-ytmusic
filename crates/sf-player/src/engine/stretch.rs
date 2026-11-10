//! Playback speed without changing pitch (WSOLA).
//!
//! The music is cut into overlapping 40 ms pieces taken from the input at
//! `speed` times the rate they are laid down at the output. Each piece is
//! nudged by up to 12 ms to where it best continues the one before, so the
//! waveforms line up and the overlap does not smear or phase. Hann halves
//! of a 50 % overlap sum to one, so a steady signal keeps its level.
//!
//! At normal speed the input passes through untouched. Going into and out
//! of a stretch is seamless: the first piece is copied as it is, and the
//! way out resumes the input exactly where the last piece would have gone
//! on.

use std::f32::consts::PI;
use std::ops::RangeInclusive;

/// The speeds the player takes.
pub const SPEED_RANGE: RangeInclusive<f32> = 0.5..=2.0;

/// Half a piece: what each step lays down at the output.
const HOP: std::time::Duration = std::time::Duration::from_millis(20);
/// How far a piece may move to line up with the one before it.
const SEEK: std::time::Duration = std::time::Duration::from_millis(12);
/// The coarse search tries every `COARSE`th offset, then refines around the
/// best of them.
const COARSE: usize = 4;

/// Speeds this close to one count as one, so a slider that lands on 1.0x
/// plays the music as it is.
fn is_normal(speed: f32) -> bool {
    (speed - 1.0).abs() < 0.005
}

pub struct TimeStretch {
    channels: usize,
    hop: usize,
    seek: usize,
    /// The rising half of the Hann window; the falling half is `1 - rise`.
    rise: Vec<f32>,
    /// Interleaved input from frame `origin` on.
    input: Vec<f32>,
    origin: usize,
    /// Where, by the speed alone, the next piece would start.
    ideal: f64,
    /// Where the last piece would go on if it were longer: the place the
    /// next piece has to sound like.
    natural: usize,
    /// The falling half of the last piece, waiting for the next to overlap.
    tail: Vec<f32>,
    /// Stretching rather than passing the input through.
    active: bool,
}

impl TimeStretch {
    pub fn new(sample_rate: u32, channels: usize) -> Self {
        let frames = |length: std::time::Duration| {
            (u64::from(sample_rate) * length.as_millis() as u64 / 1_000).max(1) as usize
        };
        let hop = frames(HOP);
        let rise = (0..hop)
            .map(|n| 0.5 - 0.5 * (PI * n as f32 / hop as f32).cos())
            .collect();
        Self {
            channels: channels.max(1),
            hop,
            seek: frames(SEEK),
            rise,
            input: Vec::new(),
            origin: 0,
            ideal: 0.0,
            natural: 0,
            tail: Vec::new(),
            active: false,
        }
    }

    /// Forgets everything held, for a seek or a new track that starts cold.
    pub fn reset(&mut self) {
        self.active = false;
        self.input.clear();
        self.tail.clear();
    }

    /// Plays `samples` at `speed`. The output is whatever the input so far
    /// allows; the last few tens of milliseconds wait for the next chunk.
    pub fn process(&mut self, samples: &[f32], speed: f32) -> Vec<f32> {
        let speed = speed.clamp(*SPEED_RANGE.start(), *SPEED_RANGE.end());
        if !self.active {
            if is_normal(speed) {
                return samples.to_vec();
            }
            self.active = true;
            self.input.clear();
            self.tail.clear();
            self.origin = 0;
            self.ideal = 0.0;
            self.natural = 0;
        }
        self.input.extend_from_slice(samples);
        let mut out = Vec::with_capacity(samples.len() * 2);
        if is_normal(speed) {
            // The tail is the falling half of the input from `natural` on,
            // so the input itself from there is the overlap made whole.
            let from = (self.natural - self.origin) * self.channels;
            out.extend_from_slice(&self.input[from..]);
            self.reset();
            return out;
        }
        let ch = self.channels;
        let hop = self.hop;
        let available = self.origin + self.input.len() / ch;
        loop {
            let ideal = self.ideal.round() as usize;
            let reach = (ideal + self.seek + 2 * hop).max(self.natural + hop);
            if reach > available {
                break;
            }
            let start = if self.tail.is_empty() {
                // The first piece: copied as it is, so the stretch starts
                // without a seam.
                let from = (ideal - self.origin) * ch;
                out.extend_from_slice(&self.input[from..from + hop * ch]);
                ideal
            } else {
                let start = self.best_start(ideal);
                let from = (start - self.origin) * ch;
                for frame in 0..hop {
                    let rise = self.rise[frame];
                    for c in 0..ch {
                        let i = frame * ch + c;
                        out.push(self.tail[i] + self.input[from + i] * rise);
                    }
                }
                start
            };
            let from = (start + hop - self.origin) * ch;
            self.tail.clear();
            for frame in 0..hop {
                let fall = 1.0 - self.rise[frame];
                for c in 0..ch {
                    self.tail.push(self.input[from + frame * ch + c] * fall);
                }
            }
            self.natural = start + hop;
            self.ideal += f64::from(speed) * hop as f64;
        }
        // Keep what the next pieces can still reach.
        let keep = (self.ideal.round() as usize)
            .saturating_sub(self.seek)
            .min(self.natural)
            .max(self.origin);
        let drop = (keep - self.origin).min(self.input.len() / ch);
        self.input.drain(..drop * ch);
        self.origin += drop;
        out
    }

    /// The start near `ideal` whose opening best matches the input at
    /// `natural`, by normalised cross-correlation of the channel sum.
    fn best_start(&self, ideal: usize) -> usize {
        let lowest = ideal.saturating_sub(self.seek).max(self.origin);
        let highest = ideal + self.seek;
        let score = |start: usize| self.similarity(start);
        let mut best = (lowest, f32::MIN);
        let mut start = lowest;
        while start <= highest {
            let value = score(start);
            if value > best.1 {
                best = (start, value);
            }
            start += COARSE;
        }
        let around = best.0;
        for start in around.saturating_sub(COARSE - 1).max(lowest)..=(around + COARSE - 1).min(highest) {
            let value = score(start);
            if value > best.1 {
                best = (start, value);
            }
        }
        best.0
    }

    fn similarity(&self, start: usize) -> f32 {
        let ch = self.channels;
        let reference = (self.natural - self.origin) * ch;
        let candidate = (start - self.origin) * ch;
        let mut dot = 0.0f32;
        let mut energy = 0.0f32;
        for frame in 0..self.hop {
            let mut a = 0.0;
            let mut b = 0.0;
            for c in 0..ch {
                a += self.input[reference + frame * ch + c];
                b += self.input[candidate + frame * ch + c];
            }
            dot += a * b;
            energy += b * b;
        }
        dot / (energy + 1e-6).sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 44_100;

    fn tone(frames: usize, hz: f32) -> Vec<f32> {
        (0..frames)
            .flat_map(|n| {
                let s = (2.0 * PI * hz * n as f32 / RATE as f32).sin() * 0.5;
                [s, s]
            })
            .collect()
    }

    fn run(stretch: &mut TimeStretch, input: &[f32], speed: f32) -> Vec<f32> {
        input
            .chunks(441 * 2)
            .flat_map(|chunk| stretch.process(chunk, speed))
            .collect()
    }

    #[test]
    fn normal_speed_passes_the_music_through_untouched() {
        let mut stretch = TimeStretch::new(RATE, 2);
        let input = tone(10_000, 440.0);
        assert_eq!(run(&mut stretch, &input, 1.0), input);
    }

    #[test]
    fn faster_and_slower_change_the_length_by_the_speed() {
        for speed in [0.5, 0.75, 1.25, 1.5, 2.0] {
            let mut stretch = TimeStretch::new(RATE, 2);
            let input = tone(RATE as usize * 4, 440.0);
            let out = run(&mut stretch, &input, speed);
            let expected = input.len() as f32 / speed;
            let ratio = out.len() as f32 / expected;
            // A few pieces wait for input that never comes.
            assert!((0.97..=1.01).contains(&ratio), "{speed}x: {ratio}");
        }
    }

    /// A steady tone keeps its level and pitch: the pieces line up, so the
    /// overlaps neither cancel nor pile up.
    #[test]
    fn a_steady_tone_keeps_its_level_and_pitch() {
        let mut stretch = TimeStretch::new(RATE, 2);
        let input = tone(RATE as usize * 2, 440.0);
        let out = run(&mut stretch, &input, 1.5);
        let left: Vec<f32> = out.iter().step_by(2).copied().collect();
        let body = &left[4_410..left.len() - 4_410];
        let rms = (body.iter().map(|s| s * s).sum::<f32>() / body.len() as f32).sqrt();
        let expected = 0.5 / 2f32.sqrt();
        assert!((rms - expected).abs() < expected * 0.05, "rms {rms}");
        let crossings = body.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        let hz = crossings as f32 / (body.len() as f32 / RATE as f32);
        assert!((hz - 440.0).abs() < 8.0, "{hz} Hz");
    }

    /// Changing speed mid-stream, including back to normal and out again,
    /// leaves no gap or jump in the output.
    #[test]
    fn going_in_and_out_of_a_stretch_is_seamless() {
        let mut stretch = TimeStretch::new(RATE, 2);
        let input = tone(RATE as usize * 3, 220.0);
        let mut out = Vec::new();
        for (index, chunk) in input.chunks(441 * 2).enumerate() {
            let speed = match index / 50 {
                1 | 4 => 1.5,
                3 => 0.75,
                _ => 1.0,
            };
            out.extend(stretch.process(chunk, speed));
        }
        let left: Vec<f32> = out.iter().step_by(2).copied().collect();
        // A 220 Hz sine at 0.5 moves at most about 0.016 per sample.
        let biggest = left
            .windows(2)
            .map(|w| (w[1] - w[0]).abs())
            .fold(0.0f32, f32::max);
        assert!(biggest < 0.03, "a jump of {biggest}");
    }

    #[test]
    fn a_reset_starts_over_at_normal_speed() {
        let mut stretch = TimeStretch::new(RATE, 2);
        run(&mut stretch, &tone(10_000, 440.0), 1.5);
        stretch.reset();
        let input = tone(2_000, 440.0);
        assert_eq!(stretch.process(&input, 1.0), input);
    }
}
