//! `signal`: writing one of `family-signals`' test signals to a WAV, so a host or a prototype in
//! another language plays exactly the samples the tests do.
//!
//! The file is mono 32-bit float, so it holds the generator's `f32` samples unchanged. A signal
//! can sound several times, summed in the order given: each repeated flag gives the next one its
//! value, and a flag given once applies to them all. So `--hz` three times with one `--at` is a
//! chord, and `--at` twelve times is a train.

use std::path::PathBuf;

use anyhow::{Result, ensure};
use family_dsp::convert::seconds_to_samples;
use family_signals::{click, note, pluck, synthesize};

use crate::write_interleaved;

/// Writes a test signal from `family-signals` to a mono 32-bit float WAV.
#[derive(Debug, clap::Args)]
pub struct Args {
    #[command(subcommand)]
    signal: Signal,
}

/// The signals, each with its parameters.
#[derive(Debug, clap::Subcommand)]
enum Signal {
    /// A sine starting at phase 0 (`family_signals::sine`).
    Sine {
        /// The frequency, in Hz.
        #[arg(long)]
        hz: f64,
        /// The peak level, 1 being full scale.
        #[arg(long, default_value_t = 1.0)]
        level: f64,
        #[command(flatten)]
        output: Output,
    },
    /// Plucked notes with their second and third harmonics and no click (`family_signals::note`).
    Note {
        /// Each note's fundamental, in Hz.
        #[arg(long = "hz", required = true)]
        pitches: Vec<f64>,
        /// When each note starts, in seconds.
        #[arg(long = "at", default_values_t = [0.0])]
        onsets: Vec<f64>,
        /// Each note's level.
        #[arg(long = "level", default_values_t = [0.3])]
        levels: Vec<f64>,
        #[command(flatten)]
        output: Output,
    },
    /// Picks' clicks, each followed by a low E (`family_signals::pluck`).
    Pluck {
        /// When each pluck starts, in seconds.
        #[arg(long = "at", default_values_t = [0.0])]
        onsets: Vec<f64>,
        #[command(flatten)]
        output: Output,
    },
    /// Picks' clicks with no note after them (`family_signals::click`).
    Click {
        /// When each click starts, in seconds.
        #[arg(long = "at", default_values_t = [0.0])]
        onsets: Vec<f64>,
        /// Each click's level.
        #[arg(long = "level", default_values_t = [0.02])]
        levels: Vec<f64>,
        #[command(flatten)]
        output: Output,
    },
    /// Uniform noise in -1 to 1 from a seeded generator (`family_signals::noise`).
    Noise {
        /// The generator's seed: the same seed gives the same noise.
        #[arg(long)]
        seed: u64,
        #[command(flatten)]
        output: Output,
    },
}

/// The file to write and how much of the signal it holds.
#[derive(Debug, clap::Args)]
struct Output {
    /// Where to write the signal.
    #[arg(long)]
    out: PathBuf,
    /// The sample rate, in Hz.
    #[arg(long, default_value_t = 48_000)]
    sample_rate: u32,
    /// The length, in seconds.
    #[arg(long, default_value_t = 1.0)]
    seconds: f64,
}

impl Output {
    /// The sample rate, which must not be 0.
    fn rate(&self) -> Result<f64> {
        ensure!(self.sample_rate > 0, "the sample rate must be above 0 Hz");
        Ok(f64::from(self.sample_rate))
    }

    /// The file's length of `signal(t)`, as [`synthesize`] samples it.
    fn synthesize(&self, signal: impl FnMut(f64) -> f64) -> Result<Vec<f32>> {
        Ok(synthesize(self.rate()?, self.seconds, signal))
    }

    /// Writes `samples` to the file.
    fn write(&self, samples: &[f32]) -> Result<()> {
        write_interleaved(
            &self.out,
            hound::WavSpec {
                channels: 1,
                sample_rate: self.sample_rate,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
            samples,
        )
    }
}

/// Writes the signal to its file.
///
/// # Errors
///
/// If the sample rate is 0, a repeated flag is given neither once nor once per sound, or the
/// file can't be written.
pub fn run(args: &Args) -> Result<()> {
    match &args.signal {
        Signal::Sine { hz, level, output } => output.write(&family_signals::sine(
            output.rate()?,
            output.seconds,
            *hz,
            *level,
        )),
        Signal::Note {
            pitches,
            onsets,
            levels,
            output,
        } => {
            let notes = pair_up([("--hz", pitches), ("--at", onsets), ("--level", levels)])?;
            output.write(&output.synthesize(|t| {
                notes
                    .iter()
                    .map(|&[hz, at, level]| note(t, at, hz, level))
                    .sum()
            })?)
        }
        Signal::Pluck { onsets, output } => {
            output.write(&output.synthesize(|t| onsets.iter().map(|&at| pluck(t, at)).sum())?)
        }
        Signal::Click {
            onsets,
            levels,
            output,
        } => {
            let clicks = pair_up([("--at", onsets), ("--level", levels)])?;
            output.write(
                &output
                    .synthesize(|t| clicks.iter().map(|&[at, level]| click(t, at, level)).sum())?,
            )
        }
        Signal::Noise { seed, output } => {
            let length = seconds_to_samples(output.seconds, output.rate()?);
            output.write(
                &family_signals::noise(*seed)
                    .take(length)
                    .collect::<Vec<_>>(),
            )
        }
    }
}

/// Each sound's values of the repeated flags `(name, values)`, in order: a flag given once applies
/// to every sound, and the others must each be given once per sound.
fn pair_up<const N: usize>(flags: [(&str, &Vec<f64>); N]) -> Result<Vec<[f64; N]>> {
    let count = flags
        .iter()
        .map(|(_, values)| values.len())
        .max()
        .unwrap_or(0);
    for (name, values) in flags {
        ensure!(
            values.len() == 1 || values.len() == count,
            "{name} is given {} times; give it once, or once for each of the {count} sounds",
            values.len()
        );
    }
    Ok((0..count)
        .map(|index| {
            flags.map(|(_, values)| {
                values
                    .get(index)
                    .or_else(|| values.first())
                    .copied()
                    .unwrap_or_default()
            })
        })
        .collect())
}
