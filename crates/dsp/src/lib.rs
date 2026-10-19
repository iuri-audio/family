//! DSP shared by the family's plugins.
//!
//! This crate is `no_std` without `alloc`, like each plugin's own `dsp` crate, so the audio path
//! cannot allocate.
#![cfg_attr(not(test), no_std)]
#![cfg_attr(
    test,
    expect(
        clippy::suboptimal_flops,
        reason = "only test builds have std's `mul_add`"
    )
)]

pub mod convert;
mod crossover;
mod di;
mod jump;
mod latency;

pub use crossover::{BUTTERWORTH_Q, Biquad, Crossover, Steepness, UnsupportedSteepness, prewarp};
pub use di::process_di;
pub use jump::{Jump, largest_jump, largest_jump_mono};
pub use latency::{ReportedLatency, lined_up};
