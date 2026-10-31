//! Reading a file down to one channel, and writing two channels as stereo.

use family_wav::{read_interleaved, read_mono, write_interleaved, write_stereo};

const fn spec(channels: u16) -> hound::WavSpec {
    hound::WavSpec {
        channels,
        sample_rate: 48_000,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    }
}

#[test]
fn reading_as_mono_averages_each_frames_channels() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stereo.wav");
    write_interleaved(&path, spec(2), &[0.5, 0.25, -1.0, 0.0]).unwrap();

    let (read, mono) = read_mono(&path).unwrap();

    assert_eq!(read.channels, 2);
    assert_eq!(mono, [0.375, -0.5]);
}

#[test]
fn writing_stereo_makes_two_channels_whatever_the_spec_says_and_cuts_to_the_shorter() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stereo.wav");

    write_stereo(&path, spec(1), &[0.5, 0.25, 0.125], &[-0.5, -0.25]).unwrap();

    let (read, samples) = read_interleaved(&path).unwrap();
    assert_eq!(read.channels, 2);
    assert_eq!(samples, [0.5, -0.5, 0.25, -0.25]);
}
