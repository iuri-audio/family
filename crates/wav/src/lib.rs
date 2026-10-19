//! WAV reading and writing for the family's offline tools, the commands every plugin's `render`
//! shares (`stats`, `split` and `join`), and the rest of what they share in [`render`].
//!
//! A plugin's `render` adds the commands to its own subcommands with [`Command`], or the `wav`
//! binary runs them on their own.

pub mod render;
pub mod split_join;
pub mod stats;

use std::path::Path;

use anyhow::{Context as _, Result};
use family_dsp::convert::{f32_to_pcm, pcm_to_f32};

/// The shared commands, for a plugin's `render` to flatten into its subcommands.
#[derive(Debug, clap::Subcommand)]
pub enum Command {
    /// Reports each channel's peak and RMS levels and largest sample-to-sample jump, for each file.
    Stats(stats::Args),
    /// Splits a stereo WAV into a mono file per side.
    Split(split_join::SplitArgs),
    /// Joins two mono WAVs into a stereo file. The shorter one is padded with silence.
    Join(split_join::JoinArgs),
}

impl Command {
    /// Runs the command.
    ///
    /// # Errors
    ///
    /// If a file can't be read or written, or doesn't suit the command.
    pub fn run(&self) -> Result<()> {
        match self {
            Self::Stats(args) => stats::run(args),
            Self::Split(args) => split_join::split(args),
            Self::Join(args) => split_join::join(args),
        }
    }
}

/// Reads a WAV file's interleaved samples as floats.
///
/// # Errors
///
/// If the file can't be opened or read as a WAV.
pub fn read_interleaved(path: &Path) -> Result<(hound::WavSpec, Vec<f32>)> {
    let mut reader =
        hound::WavReader::open(path).with_context(|| format!("can't open {}", path.display()))?;
    let spec = reader.spec();
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let bits = u32::from(spec.bits_per_sample);
            reader
                .samples::<i32>()
                .map(|s| s.map(|s| pcm_to_f32(s, bits)))
                .collect::<Result<_, _>>()
        }
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>(),
    }
    .with_context(|| format!("can't read {}", path.display()))?;
    Ok((spec, samples))
}

/// Reads a WAV file as one float channel, averaging the channels of each frame.
///
/// # Errors
///
/// If the file can't be opened or read as a WAV.
pub fn read_mono(path: &Path) -> Result<(hound::WavSpec, Vec<f32>)> {
    let (spec, samples) = read_interleaved(path)?;
    let channels = f32::from(spec.channels);
    let mono = samples
        .chunks(usize::from(spec.channels))
        .map(|frame| frame.iter().sum::<f32>() / channels)
        .collect();
    Ok((spec, mono))
}

/// Reads a WAV file as float channels, one `Vec` per channel.
///
/// # Errors
///
/// If the file can't be opened or read as a WAV.
pub fn read_channels(path: &Path) -> Result<(hound::WavSpec, Vec<Vec<f32>>)> {
    let (spec, samples) = read_interleaved(path)?;
    let count = usize::from(spec.channels);
    let mut channels = vec![Vec::with_capacity(samples.len() / count); count];
    for frame in samples.chunks(count) {
        for (channel, &sample) in channels.iter_mut().zip(frame) {
            channel.push(sample);
        }
    }
    Ok((spec, channels))
}

/// Writes interleaved float samples as a WAV with `spec`'s channel count and sample format.
///
/// # Errors
///
/// If the file can't be created or written.
pub fn write_interleaved(path: &Path, spec: hound::WavSpec, samples: &[f32]) -> Result<()> {
    let mut writer = hound::WavWriter::create(path, spec)
        .with_context(|| format!("can't create {}", path.display()))?;
    let bits = u32::from(spec.bits_per_sample);
    for &sample in samples {
        match spec.sample_format {
            hound::SampleFormat::Int => writer.write_sample(f32_to_pcm(sample, bits))?,
            hound::SampleFormat::Float => writer.write_sample(sample)?,
        }
    }
    writer.finalize()?;
    Ok(())
}

/// Writes two float channels as a stereo WAV in `spec`'s sample format. The longer channel is cut
/// to the shorter's length.
///
/// # Errors
///
/// If the file can't be created or written.
pub fn write_stereo(path: &Path, spec: hound::WavSpec, left: &[f32], right: &[f32]) -> Result<()> {
    let interleaved: Vec<f32> = left.iter().zip(right).flat_map(|(&l, &r)| [l, r]).collect();
    write_interleaved(
        path,
        hound::WavSpec {
            channels: 2,
            ..spec
        },
        &interleaved,
    )
}
