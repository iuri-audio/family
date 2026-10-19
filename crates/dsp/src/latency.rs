//! The latency a plugin reports to the host, and lining an offline render up with its DI.

/// The latency last reported to the host, in samples, so a plugin reports only the changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReportedLatency(u32);

impl ReportedLatency {
    /// A latency of `samples`, as the host takes it: one too long for it is reported as the
    /// longest it takes.
    #[must_use]
    pub fn new(samples: usize) -> Self {
        Self(u32::try_from(samples).unwrap_or(u32::MAX))
    }

    /// The latency, in samples, as the host takes it.
    #[must_use]
    pub const fn samples(self) -> u32 {
        self.0
    }

    /// Takes `samples` as the latency now, and returns it, as the host takes it, if it differs from
    /// the one last reported: the plugin reports it then, and the host compensates it anew.
    #[must_use]
    pub fn update(&mut self, samples: usize) -> Option<u32> {
        let now = Self::new(samples);
        (now != *self).then(|| {
            *self = now;
            now.0
        })
    }
}

/// What `process` gives for `di`, shifted back by its `latency` so it lines up with the DI and is
/// as long as it.
///
/// `process` is fed the latency's worth of silence past the DI's end, and its first latency's worth
/// of output is dropped.
///
/// For a processor whose delay moves, `latency` is the one it reports, its shortest, so no attack
/// comes out before the DI's.
pub fn lined_up<T>(
    di: &[f32],
    latency: usize,
    process: impl FnMut(f32) -> T,
) -> impl Iterator<Item = T> {
    di.iter()
        .copied()
        .chain(core::iter::repeat_n(0.0, latency))
        .map(process)
        .skip(latency)
}
