//! The audited float↔integer conversion helpers.

use family_dsp::convert::{
    f32_to_pcm, f64_to_f32, floor_to_count, ms_to_samples, pcm_to_f32, sample_to_seconds,
    seconds_to_samples,
};

#[test]
fn every_24_bit_sample_round_trips_exactly() {
    for sample in -(1 << 23)..(1 << 23) {
        assert_eq!(f32_to_pcm(pcm_to_f32(sample, 24), 24), sample);
    }
}

#[test]
fn every_16_bit_sample_round_trips_exactly() {
    for sample in -(1 << 15)..(1 << 15) {
        assert_eq!(f32_to_pcm(pcm_to_f32(sample, 16), 16), sample);
    }
}

#[test]
fn negative_full_scale_is_minus_one() {
    assert_eq!(pcm_to_f32(-8_388_608, 24), -1.0);
    assert_eq!(pcm_to_f32(4_194_304, 24), 0.5);
    assert_eq!(pcm_to_f32(-32_768, 16), -1.0);
}

#[test]
fn out_of_range_floats_clamp_to_the_largest_sample() {
    assert_eq!(f32_to_pcm(1.0, 24), 8_388_607);
    assert_eq!(f32_to_pcm(3.5, 24), 8_388_607);
    assert_eq!(f32_to_pcm(-3.5, 24), -8_388_608);
    assert_eq!(f32_to_pcm(1.0, 16), 32_767);
}

#[test]
fn floats_between_steps_round_to_the_nearest_step() {
    let step = 1.0 / 32_768.0;
    assert_eq!(f32_to_pcm(10.4 * step, 16), 10);
    assert_eq!(f32_to_pcm(10.6 * step, 16), 11);
    assert_eq!(f32_to_pcm(-10.6 * step, 16), -11);
}

#[test]
fn a_sample_index_converts_to_its_time_in_seconds() {
    assert_eq!(sample_to_seconds(0, 48_000), 0.0);
    assert_eq!(sample_to_seconds(24_000, 48_000), 0.5);
    assert_eq!(sample_to_seconds(441_000, 44_100), 10.0);
}

#[test]
fn an_f64_narrows_to_the_nearest_f32() {
    assert_eq!(f64_to_f32(0.5), 0.5);
    assert_eq!(f64_to_f32(1.0 + 1e-12), 1.0);
    assert_eq!(f64_to_f32(-0.1), -0.1_f32);
    assert_eq!(f64_to_f32(1e300), f32::INFINITY);
}

#[test]
fn a_duration_converts_to_the_nearest_whole_number_of_samples() {
    assert_eq!(seconds_to_samples(0.05, 48_000.0), 2_400);
    assert_eq!(seconds_to_samples(0.05, 44_100.0), 2_205);
    assert_eq!(seconds_to_samples(0.000_01, 44_100.0), 0);
    assert_eq!(seconds_to_samples(0.000_015, 44_100.0), 1);
}

#[test]
fn a_negative_or_undefined_duration_is_zero_samples() {
    assert_eq!(seconds_to_samples(-1.0, 48_000.0), 0);
    assert_eq!(seconds_to_samples(f64::NAN, 48_000.0), 0);
}

#[test]
fn a_time_in_milliseconds_converts_to_the_nearest_whole_number_of_samples() {
    assert_eq!(ms_to_samples(50.0, 48_000.0), 2_400);
    assert_eq!(ms_to_samples(1.0, 44_100.0), 44);
    assert_eq!(ms_to_samples(-1.0, 48_000.0), 0);
}

#[test]
fn a_value_rounds_down_to_a_whole_count() {
    assert_eq!(floor_to_count(2.9), 2);
    assert_eq!(floor_to_count(3.0), 3);
    assert_eq!(floor_to_count(-1.5), 0);
    assert_eq!(floor_to_count(f64::NAN), 0);
    assert_eq!(floor_to_count(f64::INFINITY), usize::MAX);
}
