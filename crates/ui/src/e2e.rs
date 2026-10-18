//! Test hooks for `cargo xtask e2e`, compiled only with the `e2e-hooks` feature. A plugin
//! forwards that feature, and only the e2e build turns it on, so a release build holds none of
//! this. Each hook does nothing unless its environment variable is set when the plugin loads.
//!
//! Every hook's variable starts with `FAMILY_E2E_`: each plugin's CI checks that its release
//! build has no such text in it.

use std::sync::OnceLock;
use std::time::Duration;

use egui::{Id, Ui};

/// `FAMILY_E2E_WHEEL=<n>`: each dial gets `n` wheel notches up, one per [`WHEEL_INTERVAL_S`],
/// starting [`WHEEL_START_S`] after the editor first draws it, as if the mouse were over it.
const WHEEL_VARIABLE: &str = "FAMILY_E2E_WHEEL";

/// Seconds after a dial is first drawn before its first notch: time for the check to start
/// playback.
const WHEEL_START_S: f64 = 2.0;

/// Seconds between notches: a quick burst, inside one wheel gesture.
const WHEEL_INTERVAL_S: f64 = 0.06;

/// How many notches `FAMILY_E2E_WHEEL` asks for, read once.
fn wheel_count() -> u32 {
    static COUNT: OnceLock<u32> = OnceLock::new();
    *COUNT.get_or_init(|| {
        std::env::var(WHEEL_VARIABLE)
            .ok()
            .and_then(|count| count.parse().ok())
            .unwrap_or(0)
    })
}

/// The notches the hook adds to the dial `dial` this frame: none unless `FAMILY_E2E_WHEEL` is
/// set.
pub fn wheel_notches(ui: &Ui, dial: Id, now: f64) -> f32 {
    let count = wheel_count();
    if count == 0 {
        return 0.0;
    }
    let key = dial.with("e2e wheel");
    let (start, mut sent): (f64, u32) = ui.data(|data| data.get_temp(key)).unwrap_or((now, 0));
    let due = f64::from(sent).mul_add(WHEEL_INTERVAL_S, WHEEL_START_S);
    let notches = if sent < count && now - start >= due {
        sent += 1;
        1.0
    } else {
        0.0
    };
    ui.data_mut(|data| data.insert_temp(key, (start, sent)));
    if sent < count {
        ui.ctx().request_repaint_after(Duration::from_millis(10));
    }
    notches
}
