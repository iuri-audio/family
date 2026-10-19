//! Turning the host's channels into a DI and writing the processed pair back, in place.

use family_dsp::process_di;

/// A stand-in for a plugin's processing that keeps every DI sample it was given and returns
/// `(di, -di)`, so each side's output shows where it went.
#[derive(Default)]
struct Recorder {
    given: Vec<f32>,
}

impl Recorder {
    fn process(&mut self, di: f32) -> (f32, f32) {
        self.given.push(di);
        (di, -di)
    }
}

#[test]
fn a_stereo_input_is_averaged_to_one_di_and_the_pair_goes_to_left_and_right() {
    let mut left = [1.0, 0.5, -1.0, 0.0];
    let mut right = [0.0, 0.5, 1.0, -0.5];
    let mut recorder = Recorder::default();

    process_di(&mut [&mut left, &mut right], true, |di| {
        recorder.process(di)
    });

    assert_eq!(recorder.given, [0.5, 0.5, 0.0, -0.25]);
    assert_eq!(left, [0.5, 0.5, 0.0, -0.25]);
    assert_eq!(right, [-0.5, -0.5, -0.0, 0.25]);
}

#[test]
fn a_mono_input_on_two_channels_is_the_first_channel() {
    let mut left = [0.25, -0.5, 1.0];
    // In a mono→stereo layout, the host's in-place buffer holds silence on the right.
    let mut right = [0.0; 3];
    let mut recorder = Recorder::default();

    process_di(&mut [&mut left, &mut right], false, |di| {
        recorder.process(di)
    });

    assert_eq!(recorder.given, [0.25, -0.5, 1.0]);
    assert_eq!(right, [-0.25, 0.5, -1.0]);
}

#[test]
fn a_single_channel_is_the_di_and_takes_the_left_of_the_pair() {
    let length = 1_000_u16;
    let ramp: Vec<f32> = (0..length).map(|n| f32::from(n) / 1_000.0).collect();
    let mut only = ramp.clone();
    let mut recorder = Recorder::default();

    process_di(&mut [&mut only], true, |di| recorder.process(di));

    assert_eq!(recorder.given, ramp);
    assert_eq!(only, ramp);
}

#[test]
fn without_channels_nothing_is_processed() {
    let mut recorder = Recorder::default();

    process_di(&mut [], true, |di| recorder.process(di));

    assert_eq!(recorder.given, [0.0_f32; 0]);
}

#[test]
fn only_the_samples_both_sides_have_are_processed() {
    let mut left = [1.0, 1.0, 1.0];
    let mut right = [1.0, 1.0];
    let mut recorder = Recorder::default();

    process_di(&mut [&mut left, &mut right], true, |di| {
        recorder.process(di)
    });

    assert_eq!(recorder.given, [1.0, 1.0]);
    assert_eq!(left, [1.0, 1.0, 1.0]);
    assert_eq!(right, [-1.0, -1.0]);
}

#[test]
fn the_mean_of_the_largest_floats_does_not_overflow() {
    let mut left = [f32::MAX, f32::MIN];
    let mut right = [f32::MAX, f32::MIN];
    let mut recorder = Recorder::default();

    process_di(&mut [&mut left, &mut right], true, |di| {
        recorder.process(di)
    });

    assert_eq!(recorder.given, [f32::MAX, f32::MIN]);
}

proptest::proptest! {
    #[test]
    fn the_same_signal_on_both_sides_averages_to_that_signal(
        signal in proptest::collection::vec(-1.0_f32..=1.0, 0..1_000),
    ) {
        let (mut left, mut right) = (signal.clone(), signal.clone());
        let mut recorder = Recorder::default();

        process_di(&mut [&mut left, &mut right], true, |di| recorder.process(di));

        proptest::prop_assert_eq!(recorder.given, signal);
    }

    #[test]
    fn an_inverted_copy_averages_to_silence(
        signal in proptest::collection::vec(-1.0_f32..=1.0, 0..1_000),
    ) {
        let mut inverted: Vec<f32> = signal.iter().map(|&s| -s).collect();
        let mut left = signal;
        let mut recorder = Recorder::default();

        process_di(&mut [&mut left, &mut inverted], true, |di| recorder.process(di));

        proptest::prop_assert!(recorder.given.iter().all(|&s| s == 0.0), "{:?}", recorder.given);
    }
}
