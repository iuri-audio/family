//! `stats`: the level and click check on any WAV file. For each channel, the peak and RMS
//! levels and the largest sample-to-sample jump, with its time.

use std::io::Write as _;
use std::path::PathBuf;

use anyhow::Result;
use family_dsp::convert::{count_to_f64, f64_to_f32, sample_to_seconds};
use family_dsp::{Jump, largest_jump};

use crate::read_channels;

/// Reports each channel's peak and RMS levels and largest sample-to-sample jump, for each file.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// The WAV files to check: mono, stereo or more channels, any sample format.
    #[arg(required = true)]
    files: Vec<PathBuf>,
}

/// Prints each file's format and length, then a line per channel.
///
/// # Errors
///
/// If a file can't be read as a WAV, or the report can't be written.
pub fn run(args: &Args) -> Result<()> {
    let mut out = std::io::stdout().lock();
    for path in &args.files {
        let (spec, channels) = read_channels(path)?;
        let length = channels.first().map_or(0, Vec::len);
        writeln!(
            out,
            "{}: {} channel{}, {} Hz, {}, {length} samples ({:.3} s)",
            path.display(),
            spec.channels,
            if spec.channels == 1 { "" } else { "s" },
            spec.sample_rate,
            format_name(spec),
            sample_to_seconds(length, spec.sample_rate)
        )?;
        for (index, samples) in channels.iter().enumerate() {
            let jump = describe_jump(largest_jump(samples, samples), spec.sample_rate);
            writeln!(
                out,
                "  {}: peak {:.3} dBFS, rms {:.3} dBFS, largest jump {jump}",
                channel_name(index, channels.len()),
                to_db(peak(samples)),
                to_db(rms(samples))
            )?;
        }
    }
    Ok(())
}

/// The sample format as people say it: `24-bit int`, `32-bit float`.
#[must_use]
pub fn format_name(spec: hound::WavSpec) -> String {
    let kind = match spec.sample_format {
        hound::SampleFormat::Int => "int",
        hound::SampleFormat::Float => "float",
    };
    format!("{}-bit {kind}", spec.bits_per_sample)
}

/// `mono` for a single channel, `left` and `right` for a pair, else `channel 1`, `channel 2`, ….
fn channel_name(index: usize, channels: usize) -> String {
    match (channels, index) {
        (1, _) => "mono".to_owned(),
        (2, 0) => "left".to_owned(),
        (2, _) => "right".to_owned(),
        _ => format!("channel {}", index + 1),
    }
}

/// The largest absolute sample.
fn peak(samples: &[f32]) -> f32 {
    samples
        .iter()
        .fold(0.0, |peak, sample| peak.max(sample.abs()))
}

/// The root mean square, 0 for no samples.
fn rms(samples: &[f32]) -> f32 {
    let sum: f64 = samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
    f64_to_f32((sum / count_to_f64(samples.len().max(1))).sqrt())
}

/// A level relative to full scale, in dB; silence is `-inf`.
fn to_db(level: f32) -> f32 {
    20.0 * level.log10()
}

/// The click statistic as the reports print it: the jump's size, and its time and sample.
#[must_use]
pub fn describe_jump(jump: Option<Jump>, sample_rate: u32) -> String {
    jump.map_or_else(
        || "none (shorter than two samples)".to_owned(),
        |jump| {
            format!(
                "{:.6} at {:.6} s (sample {})",
                jump.size,
                sample_to_seconds(jump.sample, sample_rate),
                jump.sample
            )
        },
    )
}
