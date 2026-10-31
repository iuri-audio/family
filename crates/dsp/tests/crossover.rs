//! The crossover: splits a signal into bands whose sum gives back the input.
//!
//! Frequency responses are measured exactly: feed an impulse, record each band's impulse response
//! for long enough that it has died away, and evaluate its Fourier transform at the frequency of
//! interest.
#![expect(
    clippy::unwrap_used,
    reason = "test helpers; a failure should fail the test"
)]

use family_dsp::{Crossover, Steepness, UnsupportedSteepness};

const RATES: [f64; 3] = [44_100.0, 48_000.0, 96_000.0];

const STEEPNESSES: [Steepness; 2] = [Steepness::Db12, Steepness::Db24];

/// Long enough for every band's impulse response to die away: one second at the highest sample
/// rate.
const RESPONSE_LENGTH: usize = 96_000;

/// Six bands, centred on 25, 91, 331, 1200, 4400 and 16 000 Hz: under two octaves each.
const SIX: [f32; 5] = [48.0, 174.0, 630.0, 2_300.0, 8_400.0];

/// Four bands, the middle two two and three octaves wide.
const FOUR: [f32; 3] = [200.0, 800.0, 2_500.0];

/// The impulse response of each band.
fn impulse_responses<const BANDS: usize>(
    split_hz: &[f32],
    steepness: Steepness,
    sample_rate: f64,
) -> [Vec<f64>; BANDS] {
    let mut crossover = Crossover::<BANDS>::new(split_hz, steepness, sample_rate);
    let mut responses: [Vec<f64>; BANDS] = core::array::from_fn(|_| Vec::new());
    for n in 0..RESPONSE_LENGTH {
        let input = if n == 0 { 1.0 } else { 0.0 };
        for (response, band) in responses.iter_mut().zip(crossover.process(input)) {
            response.push(f64::from(band));
        }
    }
    responses
}

/// The gain in dB of an impulse response at `frequency`.
fn gain_db(response: &[f64], frequency: f64, sample_rate: f64) -> f64 {
    let omega = core::f64::consts::TAU * frequency / sample_rate;
    let (mut re, mut im) = (0.0, 0.0);
    for (n, &h) in response.iter().enumerate() {
        let phase = omega * f64::from(u32::try_from(n).unwrap());
        re = h.mul_add(phase.cos(), re);
        im = h.mul_add(-phase.sin(), im);
    }
    20.0 * re.hypot(im).log10()
}

fn sum(responses: &[Vec<f64>]) -> Vec<f64> {
    let mut summed = vec![0.0; RESPONSE_LENGTH];
    for band in responses {
        for (total, h) in summed.iter_mut().zip(band) {
            *total += h;
        }
    }
    summed
}

/// The stated reconstruction tolerance: the summed bands are the input through an allpass filter,
/// so their gain is 0 dB at every frequency, within this many dB.
const RECONSTRUCTION_TOLERANCE_DB: f64 = 0.01;

const TEST_FREQUENCIES: [f64; 14] = [
    20.0, 48.0, 60.0, 174.0, 200.0, 333.0, 630.0, 800.0, 1_234.0, 2_300.0, 5_000.0, 8_400.0,
    12_000.0, 20_000.0,
];

fn assert_flat(summed: &[f64], label: &str, sample_rate: f64) {
    for frequency in TEST_FREQUENCIES {
        let gain = gain_db(summed, frequency, sample_rate);
        assert!(
            gain.abs() < RECONSTRUCTION_TOLERANCE_DB,
            "{label} at {sample_rate} Hz: {gain} dB at {frequency} Hz"
        );
    }
}

#[test]
fn the_summed_bands_have_the_inputs_level_at_every_frequency() {
    for steepness in STEEPNESSES {
        for sample_rate in RATES {
            let six = impulse_responses::<6>(&SIX, steepness, sample_rate);
            assert_flat(
                &sum(&six),
                &format!("six bands, {steepness:?}"),
                sample_rate,
            );
            let four = impulse_responses::<4>(&FOUR, steepness, sample_rate);
            assert_flat(
                &sum(&four),
                &format!("four bands, {steepness:?}"),
                sample_rate,
            );
            let two = impulse_responses::<2>(&[1_000.0], steepness, sample_rate);
            assert_flat(
                &sum(&two),
                &format!("two bands, {steepness:?}"),
                sample_rate,
            );
        }
    }
}

#[test]
fn neighbouring_bands_meet_at_the_split_points() {
    for steepness in STEEPNESSES {
        // 2.5 octaves apart: at 12 dB/octave, a split two octaves away still takes 0.5 dB off at
        // its neighbour's split point.
        let split_hz = [20.0, 113.0, 640.0, 3_620.0, 20_000.0];
        let responses = impulse_responses::<6>(&split_hz, steepness, 48_000.0);
        for (lower, &hz) in split_hz.iter().enumerate() {
            let below = gain_db(&responses[lower], f64::from(hz), 48_000.0);
            let above = gain_db(&responses[lower + 1], f64::from(hz), 48_000.0);
            assert!(
                (below - above).abs() < 0.5,
                "{steepness:?}: bands {lower} and {} at {hz} Hz: {below} vs {above} dB",
                lower + 1
            );
            assert!(
                (below + 6.0).abs() < 1.0,
                "{steepness:?}: band {lower} at {hz} Hz: {below} dB, expected about -6"
            );
        }
    }
}

#[test]
fn a_frequency_well_inside_a_band_lands_almost_entirely_in_that_band() {
    // At 24 dB/octave: at 12, bands under two octaves wide can't hold any frequency 12 dB clear of
    // their neighbours.
    let responses = impulse_responses::<6>(&SIX, Steepness::Db24, 48_000.0);
    for (band, frequency) in [
        (0, 25.0),
        (1, 91.0),
        (2, 331.0),
        (3, 1_200.0),
        (4, 4_400.0),
        (5, 16_000.0),
    ] {
        let gains = responses
            .each_ref()
            .map(|response| gain_db(response, frequency, 48_000.0));
        assert!(
            gains[band] > -2.0,
            "band {band} at {frequency} Hz: {gains:?} dB"
        );
        for (other, &gain) in gains.iter().enumerate().filter(|&(other, _)| other != band) {
            assert!(
                gain < gains[band] - 12.0,
                "band {other} at {frequency} Hz: {gains:?} dB"
            );
        }
    }
}

#[test]
fn the_steepness_sets_the_roll_off_per_octave() {
    // Split points far apart, so only the low split shapes band 0 between 800 and 1600 Hz.
    for (steepness, db_per_octave) in [(Steepness::Db12, 12.0), (Steepness::Db24, 24.0)] {
        let responses = impulse_responses::<4>(&[100.0, 12_000.0, 18_000.0], steepness, 96_000.0);
        let roll_off =
            gain_db(&responses[0], 800.0, 96_000.0) - gain_db(&responses[0], 1_600.0, 96_000.0);
        assert!(
            (roll_off - db_per_octave).abs() < 1.0,
            "{steepness:?}: {roll_off} dB per octave"
        );
    }
}

#[test]
fn split_points_in_any_order_give_the_same_bands() {
    let sorted = impulse_responses::<6>(&SIX, Steepness::Db24, 48_000.0);
    let shuffled = impulse_responses::<6>(
        &[2_300.0, 48.0, 8_400.0, 630.0, 174.0],
        Steepness::Db24,
        48_000.0,
    );

    assert_eq!(sorted, shuffled);
}

#[test]
fn missing_split_points_leave_the_top_bands_almost_empty_and_extra_ones_are_ignored() {
    let missing = impulse_responses::<4>(&[200.0], Steepness::Db24, 48_000.0);
    for band in &missing[2..] {
        assert!(gain_db(band, 1_000.0, 48_000.0) < -40.0);
    }
    assert_flat(&sum(&missing), "missing split points", 48_000.0);

    let extra =
        impulse_responses::<4>(&[200.0, 800.0, 2_500.0, 9_000.0], Steepness::Db24, 48_000.0);
    assert_eq!(
        extra,
        impulse_responses::<4>(&FOUR, Steepness::Db24, 48_000.0)
    );
}

#[test]
fn a_steepness_comes_from_its_decibels_per_octave() {
    assert_eq!(Steepness::try_from(12), Ok(Steepness::Db12));
    assert_eq!(Steepness::try_from(24), Ok(Steepness::Db24));
    assert_eq!(Steepness::try_from(18), Err(UnsupportedSteepness(18)));
}

#[test]
fn an_unsupported_steepness_says_what_it_was() {
    assert!(UnsupportedSteepness(18).to_string().contains("18"));
}
