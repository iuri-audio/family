//! A lamp that blinks when a counter the audio thread keeps moves, and fades out after.
//!
//! It's for an event the player should see, such as a detected attack. All timing and drawing run
//! on the editor thread, which polls the counter at [`POLL`] while the editor is open.
//!
//! Dark, it's [`LAMP_DARK`]; lit, the plugin's lit tint with the [`LAMP_GLOW`] in its colour.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use egui::epaint::Mesh;
use egui::{Pos2, Shape, Ui};

use crate::Colour;
use crate::glow::{crisp, disk};
use crate::night::{LAMP_DARK, LAMP_GLOW, LAMP_RADIUS};

/// How long the lamp takes to fade completely after the counter moves.
const FADE: Duration = Duration::from_millis(150);

/// How often the editor polls the counter while it's open: about once a frame at 60 Hz.
pub const POLL: Duration = Duration::from_millis(16);

/// A gap between polls longer than this means the editor wasn't drawing (closed, or hidden by the
/// host), so a change seen after it happened while nobody could see it, and doesn't light the
/// lamp.
const STALE: Duration = Duration::from_millis(250);

/// The lamp's brightness `since` seconds after it was lit, from 1 (just lit) to 0 (dark).
#[must_use]
pub fn brightness(since: f64) -> f32 {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the result is clamped to between 0 and 1"
    )]
    let brightness = (1.0 - since / FADE.as_secs_f64()).clamp(0.0, 1.0) as f32;
    brightness
}

/// A lamp that lights each time its counter changes.
#[derive(Debug)]
pub struct Lamp {
    /// The audio thread's counter. It only stores to it, with relaxed ordering: the count is the
    /// whole message.
    counter: Arc<AtomicU64>,
    /// The count last seen, and when it was polled, in egui's time in seconds.
    polled: Option<(u64, f64)>,
    /// When the lamp was last lit, in egui's time in seconds.
    lit_at: Option<f64>,
}

impl Lamp {
    /// A lamp that lights each time `counter` changes.
    #[must_use]
    pub const fn new(counter: Arc<AtomicU64>) -> Self {
        Self {
            counter,
            polled: None,
            lit_at: None,
        }
    }

    /// Polls the counter and draws the lamp around `centre`, then asks egui to draw again in time
    /// for the next poll, or the next step of the fade.
    pub fn show(&mut self, ui: &Ui, centre: Pos2, colour: Colour) {
        let now = ui.input(|input| input.time);
        let count = self.counter.load(Ordering::Relaxed);
        if let Some((seen, at)) = self.polled
            && count != seen
            && now - at <= STALE.as_secs_f64()
        {
            self.lit_at = Some(now);
        }
        self.polled = Some((count, now));
        let brightness = self.lit_at.map_or(0.0, |at| brightness(now - at));
        if brightness > 0.0 {
            ui.ctx().request_repaint();
        } else {
            ui.ctx().request_repaint_after(POLL);
        }

        let feather = 1.0 / ui.ctx().pixels_per_point();
        let mut mesh = Mesh::default();
        if brightness > 0.0 {
            disk(
                &mut mesh,
                centre,
                &LAMP_GLOW.profile(LAMP_RADIUS),
                colour.signal.gamma_multiply(LAMP_GLOW.opacity * brightness),
            );
        }
        disk(
            &mut mesh,
            centre,
            &crisp(LAMP_RADIUS, feather),
            LAMP_DARK.lerp_to_gamma(colour.lit, brightness),
        );
        ui.painter().add(Shape::mesh(mesh));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lamp_fades_from_full_to_dark_over_150_ms() {
        assert!((brightness(0.0) - 1.0).abs() < 1e-6);
        assert!((brightness(0.075) - 0.5).abs() < 1e-6);
        assert!(brightness(0.15).abs() < 1e-6);
        assert!(brightness(10.0).abs() < 1e-6);
    }
}
