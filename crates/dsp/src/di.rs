//! Turning the host's input into a DI, for a plugin that only takes one.

/// Processes the host's channels as one DI, in place, sample by sample.
///
/// `channels` is the host's in-place buffer, holding its input. With `stereo_input` and two
/// channels, each DI sample is the mean of left and right, so neither side is dropped; otherwise it
/// is the first channel's sample. In a mono→stereo layout the second channel holds silence, which
/// is why the layout, not the channel count, says whether to average.
///
/// `process` takes each DI sample and returns the output pair: the left goes to the first channel
/// and the right to the second. With one channel, only the left is kept. Only the samples present
/// on both sides are processed. The mean can't overflow, even of two full-scale floats at the edge
/// of the `f32` range.
pub fn process_di(
    channels: &mut [&mut [f32]],
    stereo_input: bool,
    mut process: impl FnMut(f32) -> (f32, f32),
) {
    match channels {
        [left, right, ..] => {
            for (l, r) in left.iter_mut().zip(right.iter_mut()) {
                let di = if stereo_input {
                    f32::midpoint(*l, *r)
                } else {
                    *l
                };
                (*l, *r) = process(di);
            }
        }
        [only] => {
            for sample in only.iter_mut() {
                *sample = process(*sample).0;
            }
        }
        [] => {}
    }
}
