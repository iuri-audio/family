//! `wav signal`: writing `family-signals`' test signals to a file, sample for sample.
#![expect(
    clippy::unwrap_used,
    reason = "test helpers; a failure should fail the test"
)]

use std::path::Path;
use std::process::{Command, Output};

use family_signals::{click, note, pluck, synthesize};

/// Runs `wav` with `args`, from inside `dir`.
fn wav(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wav"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn assert_failure_mentioning(output: &Output, words: &str) {
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(words), "{stderr}");
}

/// Runs `wav signal` with `args`, writing `signal.wav` in a fresh folder, and reads the file back.
fn signal(args: &[&str]) -> (hound::WavSpec, Vec<f32>) {
    let dir = tempfile::tempdir().unwrap();
    let mut command = vec!["signal"];
    command.extend_from_slice(args);
    command.extend_from_slice(&["--out", "signal.wav"]);
    let output = wav(dir.path(), &command);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut reader = hound::WavReader::open(dir.path().join("signal.wav")).unwrap();
    let samples = reader.samples::<f32>().collect::<Result<_, _>>().unwrap();
    (reader.spec(), samples)
}

/// Asserts the two signals have the same samples, bit for bit.
fn assert_same(written: &[f32], expected: &[f32]) {
    assert_eq!(written.len(), expected.len());
    let first_difference = written
        .iter()
        .zip(expected)
        .position(|(written, expected)| written.to_bits() != expected.to_bits());
    assert_eq!(first_difference, None, "the samples differ");
}

#[test]
fn the_file_is_mono_32_bit_float_at_the_sample_rate() {
    let (spec, samples) = signal(&["pluck", "--sample-rate", "44100", "--seconds", "0.5"]);
    assert_eq!(
        spec,
        hound::WavSpec {
            channels: 1,
            sample_rate: 44_100,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        }
    );
    assert_eq!(samples.len(), 22_050);
}

#[test]
fn it_is_a_second_at_48_khz_unless_told_otherwise() {
    let (spec, samples) = signal(&["pluck"]);
    assert_eq!(spec.sample_rate, 48_000);
    assert_eq!(samples.len(), 48_000);
}

#[test]
fn a_sine_is_the_sine_the_tests_play() {
    let (_, samples) = signal(&[
        "sine",
        "--hz",
        "1000",
        "--level",
        "0.25",
        "--sample-rate",
        "96000",
        "--seconds",
        "0.3",
    ]);
    assert_same(
        &samples,
        &family_signals::sine(96_000.0, 0.3, 1_000.0, 0.25),
    );
}

#[test]
fn a_note_is_the_note_the_tests_play() {
    let (_, samples) = signal(&["note", "--hz", "110", "--level", "0.4", "--at", "0.05"]);
    assert_same(
        &samples,
        &synthesize(48_000.0, 1.0, |t| note(t, 0.05, 110.0, 0.4)),
    );
}

#[test]
fn notes_at_several_pitches_sum_to_a_chord() {
    let (_, samples) = signal(&[
        "note", "--hz", "82.407", "--hz", "123.471", "--hz", "164.814", "--at", "0.1",
    ]);
    assert_same(
        &samples,
        &synthesize(48_000.0, 1.0, |t| {
            note(t, 0.1, 82.407, 0.3) + note(t, 0.1, 123.471, 0.3) + note(t, 0.1, 164.814, 0.3)
        }),
    );
}

#[test]
fn repeated_flags_pair_up_note_by_note_for_a_strummed_chord() {
    let (_, samples) = signal(&[
        "note", "--hz", "55", "--at", "0.05", "--level", "0.25", "--hz", "82.407", "--at", "0.06",
        "--level", "0.2", "--hz", "110", "--at", "0.07", "--level", "0.2",
    ]);
    assert_same(
        &samples,
        &synthesize(48_000.0, 1.0, |t| {
            note(t, 0.05, 55.0, 0.25) + note(t, 0.06, 82.407, 0.2) + note(t, 0.07, 110.0, 0.2)
        }),
    );
}

#[test]
fn notes_need_each_flag_once_or_once_per_note() {
    let dir = tempfile::tempdir().unwrap();
    let output = wav(
        dir.path(),
        &[
            "signal", "note", "--hz", "55", "--hz", "82.407", "--hz", "110", "--at", "0.05",
            "--at", "0.06", "--out", "note.wav",
        ],
    );
    assert_failure_mentioning(&output, "--at");
}

#[test]
fn clicks_can_each_have_their_own_level() {
    let (_, samples) = signal(&[
        "click", "--at", "0.01", "--level", "0.1", "--at", "0.02", "--level", "0.05",
    ]);
    assert_same(
        &samples,
        &synthesize(48_000.0, 1.0, |t| {
            click(t, 0.01, 0.1) + click(t, 0.02, 0.05)
        }),
    );
}

#[test]
fn a_pluck_is_the_pluck_the_tests_play() {
    let (_, samples) = signal(&["pluck", "--at", "0.2"]);
    assert_same(&samples, &synthesize(48_000.0, 1.0, |t| pluck(t, 0.2)));
}

#[test]
fn plucks_at_several_onsets_are_a_train() {
    let (_, samples) = signal(&["pluck", "--at", "0", "--at", "0.25", "--at", "0.5"]);
    assert_same(
        &samples,
        &synthesize(48_000.0, 1.0, |t| {
            pluck(t, 0.0) + pluck(t, 0.25) + pluck(t, 0.5)
        }),
    );
}

#[test]
fn a_click_is_the_click_the_tests_play() {
    let (_, samples) = signal(&["click", "--level", "0.1", "--at", "0.01", "--at", "0.02"]);
    assert_same(
        &samples,
        &synthesize(48_000.0, 1.0, |t| click(t, 0.01, 0.1) + click(t, 0.02, 0.1)),
    );
}

#[test]
fn noise_is_the_seeded_noise_the_tests_play() {
    let (_, samples) = signal(&["noise", "--seed", "42", "--seconds", "0.1"]);
    let expected: Vec<f32> = family_signals::noise(42).take(4_800).collect();
    assert_same(&samples, &expected);
}

#[test]
fn noise_needs_a_seed() {
    let dir = tempfile::tempdir().unwrap();
    let output = wav(dir.path(), &["signal", "noise", "--out", "noise.wav"]);
    assert_failure_mentioning(&output, "--seed");
}

#[test]
fn a_note_needs_a_pitch() {
    let dir = tempfile::tempdir().unwrap();
    let output = wav(dir.path(), &["signal", "note", "--out", "note.wav"]);
    assert_failure_mentioning(&output, "--hz");
}
