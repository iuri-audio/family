//! Test support for the family's plugins: synthetic guitar-like signals, deterministic noise, and
//! the levels tests judge them by.
//!
//! For `[dev-dependencies]`, and for `family-wav`'s offline `signal` command: it uses `std`, and
//! nothing here belongs on the audio path.
//!
//! Each generator reproduces, sample for sample, one the plugins' own tests wrote first, so a test
//! moving onto this crate plays exactly what it played before. Where two plugins wrote different
//! versions of the same idea (two picks' clicks, two plucks, two ways of sampling a sine), both
//! are here under their own names; the doc of each says how it differs from its sibling.
//!
//! The signals of time (`fn(t, ...) -> f64`, `t` in seconds) are silent before they start and add
//! together, so a test builds a DI by summing them inside [`synthesize`].
#![expect(
    clippy::suboptimal_flops,
    reason = "the signals must match the tests' original arithmetic sample for sample; `mul_add` \
              rounds differently"
)]

use core::f64::consts::TAU;

use family_dsp::convert::{bits_to_unit, count_to_f64, f64_to_f32, seconds_to_samples};

/// `seconds` of `signal(t)`, sampled at `sample_rate`: sample `n` is `signal(n / sample_rate)`,
/// narrowed to `f32`.
pub fn synthesize(sample_rate: f64, seconds: f64, mut signal: impl FnMut(f64) -> f64) -> Vec<f32> {
    (0..seconds_to_samples(seconds, sample_rate))
        .map(|index| f64_to_f32(signal(count_to_f64(index) / sample_rate)))
        .collect()
}

/// A sine at `hz`, `level` loud, `seconds` long, sampled at `sample_rate`, starting at phase 0.
///
/// Its phase at sample `n` is `TAU * hz * t` with `t = n / sample_rate`, as [`synthesize`] gives
/// it. [`sine_samples`] is the same sine computed in another order, which can differ in the last
/// bit.
#[must_use]
pub fn sine(sample_rate: f64, seconds: f64, hz: f64, level: f64) -> Vec<f32> {
    synthesize(sample_rate, seconds, |t| level * (TAU * hz * t).sin())
}

/// A sine at `hz`, `level` loud, `samples` long, sampled at `sample_rate`, starting at phase 0.
///
/// Its phase at sample `n` is `TAU * hz * n / sample_rate`, multiplied out before the division,
/// so it can differ from [`sine`]'s in the last bit. Use it to keep a test that computed its sine
/// this way playing the same samples.
#[must_use]
pub fn sine_samples(sample_rate: f64, samples: usize, hz: f64, level: f64) -> Vec<f32> {
    (0..samples)
        .map(|n| {
            let phase = TAU * hz * count_to_f64(n) / sample_rate;
            f64_to_f32(level * phase.sin())
        })
        .collect()
}

/// The click of a pick hitting the string at `at` seconds, `level` loud, with no note after it:
/// five partials from 2.3 to 11.3 kHz, building up over about half a millisecond and dying away
/// over a few.
///
/// [`slow_click`] is a softer pick: it builds up over a millisecond and adds two lower partials.
#[must_use]
pub fn click(t: f64, at: f64, level: f64) -> f64 {
    const PARTIALS_HZ: [f64; 5] = [2_300.0, 4_700.0, 6_100.0, 8_300.0, 11_300.0];
    if t < at {
        return 0.0;
    }
    let since = t - at;
    let partials: f64 = PARTIALS_HZ.iter().map(|hz| (TAU * hz * since).sin()).sum();
    level * (1.0 - (-since / 0.000_5).exp()) * (-since / 0.003).exp() * partials
}

/// The click of the string being struck at `at` seconds, `level` loud, with no note after it:
/// six partials from 1.9 to 11.3 kHz, halved, building up over about a millisecond and dying away
/// over a few.
///
/// Unlike [`click`], it builds up over a millisecond rather than half of one, reaches down to
/// 1.9 kHz, and sums its partials at half level.
#[must_use]
pub fn slow_click(t: f64, at: f64, level: f64) -> f64 {
    const PARTIALS_HZ: [f64; 6] = [1_900.0, 3_100.0, 4_700.0, 6_100.0, 8_300.0, 11_300.0];
    if t < at {
        return 0.0;
    }
    let since = t - at;
    let click: f64 = PARTIALS_HZ.iter().map(|hz| (TAU * hz * since).sin()).sum();
    level * (1.0 - (-since / 0.001).exp()) * (-since / 0.003).exp() * click / 2.0
}

/// A plucked note at `hz` from `at` seconds, `level` loud, rising over 2 ms and decaying over
/// 0.8 s.
///
/// The fundamental carries its second and third harmonics at half and quarter level. It has no
/// click; [`pluck`] adds one.
#[must_use]
pub fn note(t: f64, at: f64, hz: f64, level: f64) -> f64 {
    if t < at {
        return 0.0;
    }
    let since = t - at;
    let envelope = (since / 0.002).min(1.0) * (-since / 0.8).exp();
    let tone = (TAU * hz * since).sin()
        + 0.5 * (TAU * 2.0 * hz * since).sin()
        + 0.25 * (TAU * 3.0 * hz * since).sin();
    level * envelope * tone
}

/// A bright plucked low E (82.4 Hz) from `at` seconds, `level` loud, building up over about a
/// millisecond.
///
/// It has 24 harmonics, each at `1 / harmonic` of the fundamental and dying away `harmonic` times
/// faster than the fundamental's `decay` seconds, as a string's do. While it rings, its upper
/// harmonics make sample-to-sample changes as large as an attack's. It has no click; add
/// [`slow_click`] for one.
#[must_use]
pub fn bright_note(t: f64, at: f64, level: f64, decay: f64) -> f64 {
    if t < at {
        return 0.0;
    }
    let since = t - at;
    let harmonics: f64 = (1..=24)
        .map(|harmonic| {
            let harmonic = f64::from(harmonic);
            (-since * harmonic / decay).exp() * (TAU * 82.4 * harmonic * since).sin() / harmonic
        })
        .sum();
    level * (1.0 - (-since / 0.001).exp()) * harmonics / 2.0
}

/// A pick's [`click`] (0.02 loud) followed by a decaying low E (a [`note`] at 82.407 Hz, 0.3
/// loud), at `at` seconds.
///
/// [`quick_pluck`] is the other pluck: its note starts at once, has two harmonics and decays twice
/// as fast.
#[must_use]
pub fn pluck(t: f64, at: f64) -> f64 {
    click(t, at, 0.02) + note(t, at, 82.407, 0.3)
}

/// A pick's [`click`] (0.02 loud) followed by a decaying low E at `at` seconds: 82.4 Hz and its
/// octave at half level, 0.3 loud, starting at once and decaying over 0.4 s.
///
/// Unlike [`pluck`], the note has no 2 ms rise, no third harmonic, a fundamental of 82.4 Hz rather
/// than 82.407 Hz, and half the decay time.
#[must_use]
pub fn quick_pluck(t: f64, at: f64) -> f64 {
    if t < at {
        return 0.0;
    }
    let since = t - at;
    let note = 0.3
        * (-since / 0.4).exp()
        * ((TAU * 82.4 * since).sin() + 0.5 * (TAU * 164.8 * since).sin());
    click(t, at, 0.02) + note
}

/// The partials of a broadband note for [`broadband_note`], as `(hz, phase)` pairs: `count` of
/// them (at least two), spread evenly in pitch from 50 Hz to 8 kHz, with fixed pseudo-random
/// phases from an [`Xorshift64`].
#[must_use]
pub fn broadband_partials(count: u32) -> Vec<(f64, f64)> {
    let mut random = Xorshift64::new(0x2545_f491_4f6c_dd1d);
    (0..count)
        .map(|n| {
            let state = random.next_u64();
            let phase =
                f64::from(u32::try_from(state >> 40).unwrap_or(0)) / f64::from(1_u32 << 24) * TAU;
            (
                50.0 * (8_000.0_f64 / 50.0).powf(f64::from(n) / f64::from(count - 1)),
                phase,
            )
        })
        .collect()
}

/// A broadband note picked at `at` seconds, made of `partials` (from [`broadband_partials`]): it
/// starts sharply at 0.3 loud, scaled by the square root of the number of partials, and decays
/// over 0.4 s.
///
/// The partials' phases are fixed to `t`, not to `at`, so the note is a closed-form function of
/// time: it can be read at any fractional delay exactly.
#[must_use]
pub fn broadband_note(t: f64, at: f64, partials: &[(f64, f64)]) -> f64 {
    if t < at {
        return 0.0;
    }
    let tone: f64 = partials
        .iter()
        .map(|&(hz, phase)| (TAU * hz * t + phase).sin())
        .sum();
    0.3 * (-(t - at) / 0.4).exp() * tone / count_to_f64(partials.len()).sqrt()
}

/// A small deterministic random number generator (xorshift64, shifts 13, 7, 17), so a test plays
/// the same signal on every run.
#[derive(Debug, Clone)]
pub struct Xorshift64 {
    /// The generator's state, which is also its last output.
    state: u64,
}

impl Xorshift64 {
    /// A generator starting from `seed`, which must not be 0: from 0 it only ever gives 0.
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// The next 64 random bits.
    pub const fn next_u64(&mut self) -> u64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        self.state
    }

    /// The next value, spread evenly over `low..high`, from the top 53 of the next 64 bits.
    pub fn uniform(&mut self, low: f64, high: f64) -> f64 {
        low + (high - low) * bits_to_unit(self.next_u64())
    }
}

/// Endless uniform noise in `-1.0..1.0`, in steps of 1/32768, from an [`Xorshift64`].
///
/// Each sample is the top 16 of the generator's next 64 bits. `seed` is made odd first, so any
/// seed works, and independent streams come from different seeds (that differ above the lowest
/// bit).
///
/// [`noise32`] is the other noise: a 32-bit xorshift, seeded as given.
pub fn noise(seed: u64) -> impl Iterator<Item = f32> {
    let mut random = Xorshift64::new(seed | 1);
    core::iter::repeat_with(move || {
        let top_16_bits = u16::try_from(random.next_u64() >> 48).unwrap_or(0);
        f32::from(top_16_bits) / 32_768.0 - 1.0
    })
}

/// Endless uniform noise in `-1.0..1.0`, in steps of 1/32768, from a 32-bit xorshift (shifts
/// 13, 17, 5) seeded with `seed`, which must not be 0: each sample is the top 16 of its next 32
/// bits.
///
/// Unlike [`noise`], the generator is 32-bit and the seed is used as given.
pub fn noise32(seed: u32) -> impl Iterator<Item = f32> {
    let mut state = seed;
    core::iter::repeat_with(move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let [high, low, ..] = state.to_be_bytes();
        f32::from(u16::from_be_bytes([high, low])) / 32_768.0 - 1.0
    })
}

/// The root-mean-square level of `signal`, or 0 for an empty one.
#[must_use]
pub fn rms(signal: &[f32]) -> f64 {
    let sum: f64 = signal.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
    (sum / count_to_f64(signal.len().max(1))).sqrt()
}

/// The largest absolute sample in `signal`, or 0 for an empty one.
#[must_use]
pub fn peak(signal: &[f32]) -> f32 {
    signal.iter().fold(0.0, |peak, &s| peak.max(s.abs()))
}
