//! The dial: a 270° arc showing a parameter's value, with a dot at its end.
//!
//! Dragging up or down changes the value (Shift for fine adjustment), double-click resets it to
//! its default and the mouse wheel steps it. Every change goes through the host's parameter setter
//! inside a begin/end gesture, so hosts record it as automation. The arc reads the parameter on
//! every frame, so it follows host automation too. There's no typing a value in.
//!
//! The track is [`TRACK`], the arc up to the value the plugin's colour, optionally with the
//! [`DIAL_GLOW`], and the dot [`DOT`]. A plugin places and sizes each dial, and draws what goes
//! inside it (such as the value) itself.

use std::f32::consts::PI;

use egui::epaint::Mesh;
use egui::{Event, Id, MouseWheelUnit, Rect, Response, Sense, Shape, Ui, vec2};
use nice_plug::prelude::{Param, ParamSetter};

use crate::Colour;
use crate::glow::{crisp, disk, stroke_arc};
use crate::night::{DIAL_GLOW, DOT, TRACK};

/// How far a drag of one point moves the normalized value: a 200-point drag covers the whole
/// range.
const DRAG_PER_POINT: f32 = 0.005;

/// The same for a Shift-drag: ten times finer, so 2000 points cover the whole range.
const FINE_DRAG_PER_POINT: f32 = 0.0005;

/// How far one wheel notch moves the normalized value: 5 %.
const WHEEL_STEP: f32 = 0.05;

/// The same with Shift held: 1 %.
const FINE_WHEEL_STEP: f32 = 0.01;

/// How many points of smooth (trackpad) scrolling make one wheel notch.
const POINTS_PER_NOTCH: f32 = 50.0;

/// Where the arc starts, at the minimum: bottom left, in radians clockwise from the right on a
/// screen whose y axis points down.
const START_ANGLE: f32 = 0.75 * PI;

/// How far the arc sweeps from the minimum to the maximum: 270°.
const SWEEP: f32 = 1.5 * PI;

/// The end dot's radius as a fraction of the arc's thickness.
const DOT_RADIUS: f32 = 0.32;

/// The normalized value after dragging `rise` points upward from `value`.
///
/// A negative rise is a drag downward. Clamped to the parameter's range, so dragging back after
/// overshooting an end moves the value again straight away.
#[must_use]
pub const fn dragged(value: f32, rise: f32, fine: bool) -> f32 {
    let per_point = if fine {
        FINE_DRAG_PER_POINT
    } else {
        DRAG_PER_POINT
    };
    rise.mul_add(per_point, value).clamp(0.0, 1.0)
}

/// The normalized value after `notches` wheel notches from `value`, upward positive.
#[must_use]
pub const fn wheeled(value: f32, notches: i32, fine: bool) -> f32 {
    let step = if fine { FINE_WHEEL_STEP } else { WHEEL_STEP };
    #[expect(
        clippy::cast_precision_loss,
        reason = "a wheel event scrolls a handful of notches, far below f32's exact integer range"
    )]
    let notches = notches as f32;
    notches.mul_add(step, value).clamp(0.0, 1.0)
}

/// The angle of the arc's end for a normalized `value`, in radians clockwise from the right.
#[must_use]
pub const fn angle(value: f32) -> f32 {
    value.clamp(0.0, 1.0).mul_add(SWEEP, START_ANGLE)
}

/// The last value a dial sent to the host.
///
/// Both of nice-plug's wrappers only apply a change once the host has taken it (CLAP always, VST3
/// while the plugin is processing), so for a moment the parameter still reads its old value. Until
/// it moves off that value, the dial builds on what it sent instead, so quick drag moves and wheel
/// notches aren't lost.
#[derive(Debug, Clone, Copy)]
struct Sent {
    /// The normalized value sent.
    value: f32,
    /// The parameter's normalized value when it was sent.
    over: f32,
}

impl Sent {
    /// The normalized value to build on when the parameter reads `param`: what was last `sent`
    /// while the parameter hasn't moved off the value it had then, else `param`, which also
    /// follows host automation.
    const fn building_on(sent: Option<Self>, param: f32) -> f32 {
        match sent {
            Some(sent) if sent.over.to_bits() == param.to_bits() => sent.value,
            _ => param,
        }
    }
}

/// What a dial keeps between frames, in egui's memory under the dial's id.
#[derive(Debug, Clone, Copy, Default)]
struct State {
    /// The last value sent, if any.
    sent: Option<Sent>,
    /// Wheel movement not yet turned into a step, since trackpads scroll in fractions of a notch.
    scrolled: f32,
    /// Whether a drag holds a gesture open.
    dragging: bool,
}

/// A dial for one parameter, drawn and handled by [`Dial::show`].
#[must_use = "a dial does nothing until it's shown"]
pub struct Dial<'a, P: Param> {
    param: &'a P,
    setter: &'a ParamSetter<'a>,
    colour: Colour,
    thickness: f32,
    glow: bool,
}

impl<P: Param> std::fmt::Debug for Dial<'_, P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Dial")
            .field("param", &self.param.name())
            .finish_non_exhaustive()
    }
}

impl<'a, P: Param> Dial<'a, P> {
    /// A dial for `param`, which it sets through `setter`, with its arc in `colour` and
    /// `thickness` points thick.
    pub const fn new(
        param: &'a P,
        setter: &'a ParamSetter<'a>,
        colour: Colour,
        thickness: f32,
    ) -> Self {
        Self {
            param,
            setter,
            colour,
            thickness,
            glow: false,
        }
    }

    /// Gives the arc the glow of a lit one, for a plugin's main control.
    pub const fn glowing(mut self) -> Self {
        self.glow = true;
        self
    }

    /// Handles the mouse over `rect` and draws the dial filling it.
    #[expect(
        clippy::must_use_candidate,
        reason = "like egui's own widgets, most callers have no use for the response"
    )]
    pub fn show(self, ui: &Ui, rect: Rect) -> Response {
        let id = Id::new(("family-ui dial", self.param.name()));
        let mut state: State = ui.data(|data| data.get_temp(id)).unwrap_or_default();
        let response = ui.interact(rect, id, Sense::click_and_drag());
        let fine = ui.input(|input| input.modifiers.shift);

        if response.drag_started() && !state.dragging {
            // A drag that starts while one is still open means its release was lost (focus taken
            // mid-drag, say): carry on in that drag's gesture rather than opening a second one.
            state.dragging = true;
            self.setter.begin_set_parameter(self.param);
        }
        if response.dragged() {
            // Points are the design-size canvas's own units, so a drag covers the same share of
            // the dial at any scale.
            let rise = -response.drag_delta().y;
            if rise != 0.0 {
                let value = dragged(self.current(state), rise, fine);
                self.set(&mut state, value);
            }
        }
        if response.drag_stopped() && state.dragging {
            state.dragging = false;
            self.setter.end_set_parameter(self.param);
        }
        if response.double_clicked() {
            self.set(&mut state, self.param.default_normalized_value());
        }
        if response.hovered() {
            state.scrolled += ui.input(|input| {
                input
                    .raw
                    .events
                    .iter()
                    .map(|event| match *event {
                        Event::MouseWheel {
                            unit: MouseWheelUnit::Point,
                            delta,
                            ..
                        } => delta.y / POINTS_PER_NOTCH,
                        Event::MouseWheel { delta, .. } => delta.y,
                        _ => 0.0,
                    })
                    .sum::<f32>()
            });
            let notches = state.scrolled.trunc();
            if notches != 0.0 {
                state.scrolled -= notches;
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "a whole number of notches from one frame's wheel events fits an i32"
                )]
                let notches = notches as i32;
                let value = wheeled(self.current(state), notches, fine);
                self.set(&mut state, value);
            }
        }

        self.draw(ui, rect, self.current(state));
        ui.data_mut(|data| data.insert_temp(id, state));
        response
    }

    /// The normalized value to show and build on: the last one sent while the parameter hasn't
    /// caught up with it, else the parameter's own, which also follows host automation.
    fn current(&self, state: State) -> f32 {
        Sent::building_on(state.sent, self.param.unmodulated_normalized_value())
    }

    /// Sets the parameter to `normalized`: inside the drag's gesture while a drag holds one, or
    /// as one whole gesture of its own.
    fn set(&self, state: &mut State, normalized: f32) {
        state.sent = Some(Sent {
            value: normalized,
            over: self.param.unmodulated_normalized_value(),
        });
        let own_gesture = !state.dragging;
        if own_gesture {
            self.setter.begin_set_parameter(self.param);
        }
        self.setter.set_parameter_normalized(self.param, normalized);
        if own_gesture {
            self.setter.end_set_parameter(self.param);
        }
    }

    /// Draws the track, the arc up to `value` with its glow, and the end dot.
    fn draw(&self, ui: &Ui, rect: Rect, value: f32) {
        let feather = 1.0 / ui.ctx().pixels_per_point();
        let centre = rect.center();
        let radius = (rect.width().min(rect.height()) - self.thickness) / 2.0;
        let half_width = self.thickness / 2.0;
        let end = angle(value);
        let edge = crisp(half_width, feather);

        let mut mesh = Mesh::default();
        stroke_arc(
            &mut mesh,
            centre,
            radius,
            (START_ANGLE, angle(1.0)),
            &edge,
            TRACK,
        );
        if value > 0.0 {
            if self.glow {
                stroke_arc(
                    &mut mesh,
                    centre,
                    radius,
                    (START_ANGLE, end),
                    &DIAL_GLOW.profile(half_width),
                    self.colour.signal.gamma_multiply(DIAL_GLOW.opacity),
                );
            }
            stroke_arc(
                &mut mesh,
                centre,
                radius,
                (START_ANGLE, end),
                &edge,
                self.colour.signal,
            );
        }
        disk(
            &mut mesh,
            centre + radius * vec2(end.cos(), end.sin()),
            &crisp(self.thickness * DOT_RADIUS, feather),
            DOT,
        );
        ui.painter().add(Shape::mesh(mesh));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 1e-6,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn dragging_up_200_points_covers_the_whole_range() {
        assert_close(dragged(0.0, 200.0, false), 1.0);
    }

    #[test]
    fn dragging_down_lowers_the_value() {
        assert_close(dragged(0.5, -20.0, false), 0.4);
    }

    #[test]
    fn a_fine_drag_moves_a_tenth_as_far() {
        assert_close(
            dragged(0.5, 20.0, true) - 0.5,
            (dragged(0.5, 20.0, false) - 0.5) / 10.0,
        );
    }

    #[test]
    fn dragging_past_either_end_stops_there() {
        assert_close(dragged(0.9, 100.0, false), 1.0);
        assert_close(dragged(0.1, -100.0, false), 0.0);
    }

    #[test]
    fn a_wheel_notch_steps_5_percent_or_1_percent_with_shift() {
        assert_close(wheeled(0.5, 1, false), 0.55);
        assert_close(wheeled(0.5, -2, false), 0.4);
        assert_close(wheeled(0.5, 1, true), 0.51);
    }

    #[test]
    fn wheeling_past_either_end_stops_there() {
        assert_close(wheeled(0.98, 1, false), 1.0);
        assert_close(wheeled(0.02, -1, false), 0.0);
    }

    #[test]
    fn a_sent_value_is_built_on_until_the_parameter_moves_off_its_old_value() {
        let sent = Some(Sent {
            value: 0.6,
            over: 0.5,
        });

        // The host hasn't taken the change yet.
        assert_close(Sent::building_on(sent, 0.5), 0.6);
        // It has.
        assert_close(Sent::building_on(sent, 0.6), 0.6);
        // Automation has moved the parameter since.
        assert_close(Sent::building_on(sent, 0.2), 0.2);
        // Nothing sent yet.
        assert_close(Sent::building_on(None, 0.3), 0.3);
    }

    #[test]
    fn the_arc_runs_270_degrees_clockwise_from_the_bottom_left() {
        assert_close(angle(0.0).to_degrees(), 135.0);
        assert_close(angle(0.5).to_degrees(), 270.0);
        assert_close(angle(1.0).to_degrees(), 405.0);
    }
}
