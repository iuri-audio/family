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
    use egui::{Color32, FullOutput, RawInput, ViewportId, pos2};

    use super::*;

    const CENTRE: Pos2 = pos2(50.0, 50.0);
    const COLOUR: Colour = Colour {
        signal: Color32::GREEN,
        lit: Color32::YELLOW,
    };

    struct Rig {
        ctx: egui::Context,
        lamp: Lamp,
        counter: Arc<AtomicU64>,
    }

    impl Rig {
        /// A lamp that has polled its counter, already at 7 from before, for the first time at
        /// `time`.
        fn polled_at(time: f64) -> Self {
            let counter = Arc::new(AtomicU64::new(7));
            let mut rig = Self {
                ctx: egui::Context::default(),
                lamp: Lamp::new(Arc::clone(&counter)),
                counter,
            };
            rig.frame(time);
            rig
        }

        fn frame(&mut self, time: f64) -> FullOutput {
            let mut output = self.ctx.run_ui(
                RawInput {
                    time: Some(time),
                    // egui takes this off a requested delay, and tests read the delay.
                    predicted_dt: 0.0,
                    ..Default::default()
                },
                |ui| self.lamp.show(ui, CENTRE, COLOUR),
            );
            output.textures_delta.clear();
            output
        }

        fn event(&self) {
            self.counter.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn vertices(output: &FullOutput) -> &[egui::epaint::Vertex] {
        let mesh = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                Shape::Mesh(mesh) => Some(mesh),
                _ => None,
            })
            .expect("the lamp painted");
        &mesh.vertices
    }

    /// The colour at the lamp's middle: the last disk's first vertex, its centre.
    fn core(output: &FullOutput) -> Color32 {
        let vertices = vertices(output);
        let (index, _) = vertices
            .iter()
            .enumerate()
            .rfind(|(_, vertex)| vertex.pos == CENTRE)
            .expect("a disk around the centre");
        vertices[index].color
    }

    /// How far the lamp's paint reaches from its centre.
    fn reach(output: &FullOutput) -> f32 {
        vertices(output)
            .iter()
            .map(|vertex| (vertex.pos - CENTRE).length())
            .fold(0.0, f32::max)
    }

    fn repaint_delay(output: &FullOutput) -> Duration {
        output.viewport_output[&ViewportId::ROOT].repaint_delay
    }

    #[test]
    fn a_count_from_before_the_editor_opened_doesnt_light_it() {
        let mut rig = Rig::polled_at(1.0);

        let output = rig.frame(1.016);

        assert_eq!(core(&output), LAMP_DARK);
        assert!((reach(&output) - LAMP_RADIUS).abs() < 1.0, "no glow");
    }

    #[test]
    fn a_dark_lamp_polls_at_its_interval() {
        let mut rig = Rig::polled_at(1.0);
        rig.frame(1.016);
        rig.frame(1.032);

        assert_eq!(repaint_delay(&rig.frame(1.048)), POLL);
    }

    #[test]
    fn it_lights_with_a_glow_when_the_counter_moves_and_redraws_to_fade() {
        let mut rig = Rig::polled_at(1.0);
        let dark = rig.frame(1.016);
        rig.event();

        let lit = rig.frame(1.032);

        assert_eq!(core(&lit), COLOUR.lit);
        assert!(reach(&lit) > reach(&dark) + 1.0);
        assert_eq!(repaint_delay(&lit), Duration::ZERO);
    }

    #[test]
    fn the_lamp_and_its_glow_fade_together_and_it_ends_dark() {
        let mut rig = Rig::polled_at(1.0);
        rig.event();
        let lit = rig.frame(1.016);
        let glow = |output: &FullOutput| vertices(output)[0].color.a();
        let full = glow(&lit);

        let halfway = rig.frame(1.016 + FADE.as_secs_f64() / 2.0);
        assert_ne!(core(&halfway), COLOUR.lit);
        assert_ne!(core(&halfway), LAMP_DARK);
        assert!(glow(&halfway).abs_diff(full / 2) <= 2, "{}", glow(&halfway));

        assert_eq!(core(&rig.frame(2.0)), LAMP_DARK);
        // egui draws one more frame after a request for an immediate one.
        assert_eq!(repaint_delay(&rig.frame(2.016)), POLL);
    }

    #[test]
    fn a_change_seen_after_the_editor_wasnt_drawing_doesnt_light_it() {
        let mut rig = Rig::polled_at(1.0);
        rig.event();

        let reopened = rig.frame(STALE.as_secs_f64().mul_add(2.0, 1.0));

        assert_eq!(core(&reopened), LAMP_DARK);
    }

    #[test]
    fn its_edge_is_one_physical_pixel_soft_at_any_scale() {
        for pixels_per_point in [1.0, 2.0] {
            let mut rig = Rig::polled_at(1.0);
            rig.ctx.set_pixels_per_point(pixels_per_point);
            rig.frame(1.016);

            let output = rig.frame(1.032);

            assert!(
                (reach(&output) - (LAMP_RADIUS + 0.5 / pixels_per_point)).abs() < 1e-3,
                "{}",
                reach(&output)
            );
        }
    }

    #[test]
    fn the_lamp_fades_from_full_to_dark_over_150_ms() {
        assert!((brightness(0.0) - 1.0).abs() < 1e-6);
        assert!((brightness(0.075) - 0.5).abs() < 1e-6);
        assert!(brightness(0.15).abs() < 1e-6);
        assert!(brightness(10.0).abs() < 1e-6);
    }
}
