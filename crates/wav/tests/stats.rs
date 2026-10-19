//! `wav stats`: the level and click check on any WAV file, per channel.
#![expect(
    clippy::unwrap_used,
    reason = "test helpers; a failure should fail the test"
)]

use std::path::Path;
use std::process::{Command, Output};

use family_dsp::Jump;
use family_wav::stats::describe_jump;

fn write_wav<S: hound::Sample + Copy>(path: &Path, spec: hound::WavSpec, samples: &[S]) {
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    for &sample in samples {
        writer.write_sample(sample).unwrap();
    }
    writer.finalize().unwrap();
}

/// Runs `wav stats` on `files`, from inside `dir`.
fn stats(dir: &Path, files: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wav"))
        .current_dir(dir)
        .arg("stats")
        .args(files)
        .output()
        .unwrap()
}

/// The stdout of a successful run.
fn stdout(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// One channel's line of the report.
#[derive(Debug, PartialEq)]
struct Channel {
    peak: String,
    rms: String,
    jump: String,
    at: String,
    sample: String,
}

/// The line for `channel` in the section for `file`.
fn channel(stdout: &str, file: &str, channel: &str) -> Channel {
    let line = stdout
        .lines()
        .skip_while(|line| !line.starts_with(&format!("{file}:")))
        .skip(1)
        .take_while(|line| line.starts_with(' '))
        .find(|line| line.trim_start().starts_with(&format!("{channel}:")))
        .expect("a line for the channel");
    let after = |word: &str| {
        let (_, rest) = line.split_once(word).unwrap();
        rest.split_whitespace()
            .next()
            .unwrap()
            .trim_matches(|c| c == ',' || c == ')')
            .to_owned()
    };
    Channel {
        peak: after("peak "),
        rms: after("rms "),
        jump: after("largest jump "),
        at: after(" at "),
        sample: after("(sample "),
    }
}

#[test]
fn reports_peak_rms_and_largest_jump_of_a_mono_file() {
    let dir = tempfile::tempdir().unwrap();
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 1_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    // Peak 0.5 (-6.021 dBFS); RMS √(0.5625 / 8) = 0.265 (-11.530 dBFS); the largest jump is
    // from +0.5 to -0.5, landing on sample 2.
    write_wav(
        &dir.path().join("mono.wav"),
        spec,
        &[0_i16, 16_384, -16_384, 0, 8_192, 0, 0, 0],
    );

    let stdout = stdout(stats(dir.path(), &["mono.wav"]));

    assert!(
        stdout.contains("mono.wav: 1 channel, 1000 Hz, 16-bit int, 8 samples (0.008 s)"),
        "{stdout}"
    );
    assert_eq!(
        channel(&stdout, "mono.wav", "mono"),
        Channel {
            peak: "-6.021".into(),
            rms: "-11.530".into(),
            jump: "1.000000".into(),
            at: "0.002000".into(),
            sample: "2".into(),
        },
        "{stdout}"
    );
}

#[test]
fn reports_each_channel_of_a_stereo_file_and_each_file() {
    let dir = tempfile::tempdir().unwrap();
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 1_000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    // Left: a steady 0.25 (-12.041 dBFS), so no jump. Right: 0, -1, +1, 0: peak 0 dBFS, RMS
    // √0.5 (-3.010 dBFS), the largest jump 2 on sample 2.
    write_wav(
        &dir.path().join("stereo.wav"),
        spec,
        &[0.25_f32, 0.0, 0.25, -1.0, 0.25, 1.0, 0.25, 0.0],
    );
    write_wav(
        &dir.path().join("silent.wav"),
        hound::WavSpec {
            channels: 1,
            ..spec
        },
        &[0.0_f32; 3],
    );

    let stdout = stdout(stats(dir.path(), &["stereo.wav", "silent.wav"]));

    assert_eq!(
        channel(&stdout, "stereo.wav", "left"),
        Channel {
            peak: "-12.041".into(),
            rms: "-12.041".into(),
            jump: "0.000000".into(),
            at: "0.001000".into(),
            sample: "1".into(),
        },
        "{stdout}"
    );
    assert_eq!(
        channel(&stdout, "stereo.wav", "right"),
        Channel {
            peak: "0.000".into(),
            rms: "-3.010".into(),
            jump: "2.000000".into(),
            at: "0.002000".into(),
            sample: "2".into(),
        },
        "{stdout}"
    );
    let silent = channel(&stdout, "silent.wav", "mono");
    assert_eq!(
        (silent.peak.as_str(), silent.rms.as_str()),
        ("-inf", "-inf")
    );
}

#[test]
fn a_missing_file_is_an_error_naming_it() {
    let dir = tempfile::tempdir().unwrap();

    let output = stats(dir.path(), &["nowhere.wav"]);

    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("nowhere.wav")
    );
}

#[test]
fn a_jump_is_described_with_its_time() {
    let jump = Jump {
        size: 0.5,
        sample: 24_000,
    };

    assert_eq!(
        describe_jump(Some(jump), 48_000),
        "0.500000 at 0.500000 s (sample 24000)"
    );
}

#[test]
fn no_jump_is_described_as_too_short_a_signal() {
    assert_eq!(
        describe_jump(None, 48_000),
        "none (shorter than two samples)"
    );
}
