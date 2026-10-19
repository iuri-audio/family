//! The latency a plugin reports to the host, and lining an offline render up with its DI.

use std::collections::VecDeque;

use family_dsp::{ReportedLatency, lined_up};

#[test]
fn a_new_latency_is_reported_as_given() {
    assert_eq!(ReportedLatency::new(480).samples(), 480);
}

#[test]
fn an_unchanged_latency_is_not_reported_again() {
    let mut latency = ReportedLatency::new(480);

    assert_eq!(latency.update(480), None);
    assert_eq!(latency.samples(), 480);
}

#[test]
fn a_changed_latency_is_reported_once() {
    let mut latency = ReportedLatency::new(480);

    assert_eq!(latency.update(0), Some(0));
    assert_eq!(latency.update(0), None);
    assert_eq!(latency.samples(), 0);
}

#[test]
fn a_latency_too_long_for_the_host_is_reported_as_the_longest_it_takes() {
    let too_long = usize::try_from(u64::from(u32::MAX) + 1).unwrap_or(usize::MAX);

    assert_eq!(ReportedLatency::new(too_long).samples(), u32::MAX);
}

/// A processor that only delays its input by `latency` samples.
fn delay(latency: usize) -> impl FnMut(f32) -> f32 {
    let mut line: VecDeque<f32> = std::iter::repeat_n(0.0, latency).collect();
    move |sample| {
        line.push_back(sample);
        line.pop_front().unwrap_or(0.0)
    }
}

#[test]
fn a_render_is_shifted_back_by_the_latency() {
    let di = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6];

    let rendered: Vec<f32> = lined_up(&di, 4, delay(4)).collect();

    assert_eq!(rendered, di);
}

#[test]
fn a_render_without_latency_is_the_processors_output() {
    let di = [0.1, -0.2, 0.3];

    let rendered: Vec<f32> = lined_up(&di, 0, |sample| -sample).collect();

    assert_eq!(rendered, [-0.1, 0.2, -0.3]);
}

#[test]
fn the_processor_is_fed_the_di_then_the_latencys_worth_of_silence() {
    let di = [0.1, 0.2, 0.3];
    let mut given = Vec::new();

    let rendered: Vec<(f32, f32)> = lined_up(&di, 2, |sample| {
        given.push(sample);
        (sample, -sample)
    })
    .collect();

    assert_eq!(given, [0.1, 0.2, 0.3, 0.0, 0.0]);
    assert_eq!(rendered, [(0.3, -0.3), (0.0, -0.0), (0.0, -0.0)]);
}
