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
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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

/// Seconds to let the host apply a change after input ends. A refused change, or a touch-mode
/// return between frames, must not leave the arc stuck on the requested value.
const HOST_GRACE_SECONDS: f64 = 0.1;

/// Keep nearby wheel notches in one gesture. A same-frame begin/value/end can leave Reaper's
/// touch envelope recording only its old value, so let the host observe the edit before ending it.
const WHEEL_IDLE_SECONDS: f64 = 0.15;

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
/// notches aren't lost. Retire it when the host moves the parameter, or shortly after the
/// gesture ends, so returning to the original value cannot revive it.
#[derive(Debug, Clone, Copy)]
struct Sent {
    /// The normalized value sent.
    value: f32,
    /// The parameter's normalized value when it was sent.
    over: f32,
    /// Time of the last input or the end of its drag, in egui's clock.
    at: f64,
}

impl Sent {
    /// Builds on a pending change while the host catches up. Once acknowledged or expired,
    /// forget it permanently, including if automation later returns to the original value.
    fn building_on(sent: &mut Option<Self>, param: f32, now: f64, dragging: bool) -> f32 {
        if let Some(pending) = *sent {
            if pending.over.to_bits() == param.to_bits()
                && (dragging || now - pending.at < HOST_GRACE_SECONDS)
            {
                return pending.value;
            }
            *sent = None;
        }
        param
    }
}

/// What a dial keeps between frames, in egui's memory under the dial's id.
#[derive(Debug, Clone, Default)]
struct State {
    /// The last value sent, if any.
    sent: Option<Sent>,
    /// Wheel movement not yet turned into a step, since trackpads scroll in fractions of a notch.
    scrolled: f32,
    /// Whether a drag holds a gesture open.
    dragging: bool,
    /// End a wheel burst after this time, giving the host time to record it.
    wheel_until: Option<f64>,
    /// Last-owner cleanup when the window's egui memory is dropped during a gesture.
    cleanup: Option<Arc<GestureCleanup>>,
}

struct GestureCleanup {
    active: AtomicBool,
    finish: Arc<dyn Fn() + Send + Sync>,
}

impl std::fmt::Debug for GestureCleanup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GestureCleanup").finish_non_exhaustive()
    }
}

impl Drop for GestureCleanup {
    fn drop(&mut self) {
        if self.active.load(Ordering::Relaxed) {
            (self.finish)();
        }
    }
}

impl State {
    fn disarm_cleanup(&mut self) {
        if let Some(cleanup) = self.cleanup.take() {
            cleanup.active.store(false, Ordering::Relaxed);
        }
    }
}

/// A dial for one parameter, drawn and handled by [`Dial::show`].
#[must_use = "a dial does nothing until it's shown"]
pub struct Dial<'a, P: Param> {
    param: &'a P,
    setter: &'a ParamSetter<'a>,
    colour: Colour,
    thickness: f32,
    glow: bool,
    on_close: Option<Arc<dyn Fn() + Send + Sync>>,
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
            on_close: None,
        }
    }

    /// Ends an unfinished gesture when the editor's egui memory is dropped. The callback must
    /// enqueue the end on the host's GUI executor, retaining the parameters until it runs; egui
    /// memory can be dropped on another thread. It must not capture the egui context itself.
    ///
    /// Install this for editors that can close during a wheel burst or drag.
    pub fn on_close(mut self, finish: impl Fn() + Send + Sync + 'static) -> Self {
        self.on_close = Some(Arc::new(finish));
        self
    }

    fn begin(&self, state: &mut State) {
        self.setter.begin_set_parameter(self.param);
        state.cleanup = self.on_close.as_ref().map(|finish| {
            Arc::new(GestureCleanup {
                active: AtomicBool::new(true),
                finish: finish.clone(),
            })
        });
    }

    fn end(&self, state: &mut State) {
        state.disarm_cleanup();
        self.setter.end_set_parameter(self.param);
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
        let (fine, now) = ui.input(|input| (input.modifiers.shift, input.time));

        if state.wheel_until.is_some_and(|until| now >= until) {
            state.wheel_until = None;
            self.end(&mut state);
        }
        if response.drag_started() && !state.dragging {
            // A drag that starts while one is still open means its release was lost (focus taken
            // mid-drag, say): carry on in that drag's gesture rather than opening a second one.
            state.dragging = true;
            if state.wheel_until.take().is_none() {
                self.begin(&mut state);
            }
        }
        if response.dragged() {
            // Points are the design-size canvas's own units, so a drag covers the same share of
            // the dial at any scale.
            let rise = -response.drag_delta().y;
            if rise != 0.0 {
                let value = dragged(self.current(&mut state, now), rise, fine);
                self.set(&mut state, value, now);
            }
        }
        if response.drag_stopped() && state.dragging {
            state.dragging = false;
            if let Some(sent) = &mut state.sent {
                sent.at = now;
            }
            self.end(&mut state);
        }
        if response.double_clicked() {
            self.set(&mut state, self.param.default_normalized_value(), now);
        }
        #[cfg(feature = "e2e-hooks")]
        let hooked = crate::e2e::wheel_notches(ui, id, now);
        #[cfg(not(feature = "e2e-hooks"))]
        let hooked = 0.0;
        if response.hovered() || hooked != 0.0 {
            state.scrolled += hooked
                + ui.input(|input| {
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
                if !state.dragging {
                    if state.wheel_until.is_none() {
                        self.begin(&mut state);
                    }
                    state.wheel_until = Some(now + WHEEL_IDLE_SECONDS);
                }
                let value = wheeled(self.current(&mut state, now), notches, fine);
                self.set(&mut state, value, now);
            }
        }

        self.draw(ui, rect, self.current(&mut state, now));
        if state.sent.is_some() {
            // Also settle refused changes when the pointer stops moving.
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(HOST_GRACE_SECONDS));
        }
        if let Some(until) = state.wheel_until {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64((until - now).max(0.0)));
        }
        ui.data_mut(|data| data.insert_temp(id, state));
        response
    }

    /// The normalized value to show and build on: the last one sent while the parameter hasn't
    /// caught up with it, else the parameter's own, which also follows host automation.
    fn current(&self, state: &mut State, now: f64) -> f32 {
        Sent::building_on(
            &mut state.sent,
            self.param.unmodulated_normalized_value(),
            now,
            state.dragging,
        )
    }

    /// Sets the parameter to `normalized`: inside the drag's gesture while a drag holds one, or
    /// as one whole gesture of its own.
    fn set(&self, state: &mut State, normalized: f32, now: f64) {
        state.sent = Some(Sent {
            value: normalized,
            over: self.param.unmodulated_normalized_value(),
            at: now,
        });
        let own_gesture = !state.dragging && state.wheel_until.is_none();
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
    use nice_plug::prelude::{FloatParam, FloatRange};

    use crate::fake_host::Host;

    /// Runs the real widget with a host-controlled parameter, and returns the painted dot's x
    /// coordinate. Fresh parameters keep the fake host independent of nice-plug's unsafe setters;
    /// the widget's state persists under its stable name, just as it does in an editor.
    fn frame(ctx: &egui::Context, host: &Host, time: f64, value: f32, events: Vec<Event>) -> f32 {
        let output = run(ctx, host, time, value, events);
        let (min, max) = extent_of(&output, DOT);
        min.midpoint(max)
    }

    /// One frame of the dial in [`RECT`], `THICKNESS` thick, with its parameter at `value`.
    fn run(
        ctx: &egui::Context,
        host: &Host,
        time: f64,
        value: f32,
        events: Vec<Event>,
    ) -> egui::FullOutput {
        let param = FloatParam::new("Width", value, FloatRange::Linear { min: 0.0, max: 1.0 });
        let setter = ParamSetter { raw_context: host };
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(200.0, 200.0))),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ui| {
                Dial::new(
                    &param,
                    &setter,
                    Colour {
                        signal: egui::Color32::GREEN,
                        lit: egui::Color32::GREEN,
                    },
                    THICKNESS,
                )
                .on_close({
                    let gestures = host.gestures.clone();
                    move || gestures.lock().expect("gestures").push("end")
                })
                .show(ui, RECT);
            },
        );
        output.textures_delta.clear();
        output
    }

    const THICKNESS: f32 = 4.0;
    const RECT: Rect = Rect {
        min: egui::pos2(20.0, 20.0),
        max: egui::pos2(180.0, 180.0),
    };

    /// The vertices painted fully in `colour`.
    fn painted(output: &egui::FullOutput, colour: egui::Color32) -> Vec<egui::Pos2> {
        output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                Shape::Mesh(mesh) => Some(mesh.vertices.iter()),
                _ => None,
            })
            .flatten()
            .filter(|v| v.color == colour)
            .map(|v| v.pos)
            .collect()
    }

    /// The left and right ends of what's painted fully in `colour`.
    fn extent_of(output: &egui::FullOutput, colour: egui::Color32) -> (f32, f32) {
        let (min, max) = painted(output, colour)
            .iter()
            .fold((f32::INFINITY, f32::NEG_INFINITY), |(min, max), pos| {
                (min.min(pos.x), max.max(pos.x))
            });
        assert!(min.is_finite() && max.is_finite(), "nothing in {colour:?}");
        (min, max)
    }

    const GREEN: egui::Color32 = egui::Color32::GREEN;

    fn wheel() -> Vec<Event> {
        vec![
            Event::PointerMoved(egui::pos2(100.0, 100.0)),
            Event::MouseWheel {
                unit: MouseWheelUnit::Line,
                phase: egui::TouchPhase::Move,
                delta: vec2(0.0, 1.0),
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }

    fn scroll(at: egui::Pos2, unit: MouseWheelUnit, y: f32) -> Vec<Event> {
        vec![
            Event::PointerMoved(at),
            Event::MouseWheel {
                unit,
                phase: egui::TouchPhase::Move,
                delta: vec2(0.0, y),
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }

    fn click_at(ctx: &egui::Context, host: &Host, time: f64, value: f32, at: egui::Pos2) {
        let button = |pressed| Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(
            ctx,
            host,
            time,
            value,
            vec![Event::PointerMoved(at), button(true)],
        );
        frame(ctx, host, time + 0.01, value, vec![button(false)]);
    }

    #[test]
    fn the_wheel_only_moves_the_dial_under_the_pointer() {
        let ctx = egui::Context::default();
        let host = Host::default();
        frame(&ctx, &host, 0.0, 0.5, vec![]);

        let outside = egui::pos2(5.0, 5.0);
        frame(
            &ctx,
            &host,
            0.01,
            0.5,
            scroll(outside, MouseWheelUnit::Line, 1.0),
        );

        assert!(host.requests.lock().expect("requests").is_empty());
    }

    #[test]
    fn a_trackpads_smooth_scrolling_adds_up_to_notches() {
        let ctx = egui::Context::default();
        let host = Host::default();
        let over = egui::pos2(100.0, 100.0);
        frame(&ctx, &host, 0.0, 0.5, vec![]);

        let half_a_notch = POINTS_PER_NOTCH / 2.0;
        frame(
            &ctx,
            &host,
            0.01,
            0.5,
            scroll(over, MouseWheelUnit::Point, half_a_notch),
        );
        assert!(host.requests.lock().expect("requests").is_empty());
        frame(
            &ctx,
            &host,
            0.02,
            0.5,
            scroll(over, MouseWheelUnit::Point, half_a_notch),
        );

        let requests = host.requests.lock().expect("requests");
        assert_eq!(requests.len(), 1);
        assert_close(requests[0], 0.55);
    }

    #[test]
    fn double_clicking_resets_to_the_default_inside_one_gesture() {
        let ctx = egui::Context::default();
        let host = Host::default();
        let over = egui::pos2(100.0, 100.0);
        frame(&ctx, &host, 0.0, 0.5, vec![]);

        click_at(&ctx, &host, 0.01, 0.5, over);
        click_at(&ctx, &host, 0.05, 0.5, over);

        let requests = host.requests.lock().expect("requests").clone();
        assert_eq!(requests.len(), 1);
        assert_close(requests[0], 0.5);
        assert_eq!(*host.gestures.lock().expect("gestures"), ["begin", "end"]);
    }

    #[test]
    fn a_wheel_gesture_gets_a_frame_when_it_is_due_to_end() {
        let ctx = egui::Context::default();
        let host = Host::default();
        frame(&ctx, &host, 9.0, 0.5, vec![]);
        frame(&ctx, &host, 9.5, 0.5, vec![]);
        // A gesture already open, since a real notch has egui draw frames for its own smooth
        // scrolling for longer than the gesture lasts.
        ctx.data_mut(|data| {
            data.insert_temp(
                Id::new(("family-ui dial", "Width")),
                State {
                    wheel_until: Some(10.16),
                    ..State::default()
                },
            );
        });

        let output = run(&ctx, &host, 10.11, 0.5, vec![]);

        let delay = output.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
        // egui takes its predicted frame time off a requested delay.
        let left = 10.16 - 10.11 - 1.0 / 60.0;
        assert!((delay.as_secs_f64() - left).abs() < 0.01, "{delay:?}");
    }

    /// The nearest and furthest that `points` get from the dial's centre.
    fn reach(points: &[egui::Pos2]) -> (f32, f32) {
        points
            .iter()
            .map(|&pos| (pos - RECT.center()).length())
            .fold((f32::INFINITY, 0.0), |(min, max), d| {
                (min.min(d), max.max(d))
            })
    }

    #[test]
    fn the_track_fills_the_rect_and_its_edge_is_one_physical_pixel_soft_at_any_scale() {
        for pixels_per_point in [1.0, 2.0] {
            let ctx = egui::Context::default();
            ctx.set_pixels_per_point(pixels_per_point);
            let host = Host::default();
            frame(&ctx, &host, 0.0, 0.5, vec![]);
            let output = run(&ctx, &host, 0.01, 0.5, vec![]);

            let (inner, outer) = reach(&painted(&output, TRACK));

            // Fully opaque up to half a physical pixel inside the stroke's edges.
            let half_pixel = 0.5 / pixels_per_point;
            let edge = RECT.width() / 2.0;
            assert!((outer - (edge - half_pixel)).abs() < 1e-3, "{outer}");
            assert!(
                (inner - (edge - THICKNESS + half_pixel)).abs() < 1e-3,
                "{inner}"
            );
        }
    }

    #[test]
    fn the_arc_shows_only_above_the_minimum() {
        let ctx = egui::Context::default();
        let host = Host::default();
        frame(&ctx, &host, 0.0, 0.0, vec![]);

        assert_eq!(painted(&run(&ctx, &host, 1.0, 0.0, vec![]), GREEN), []);
        assert_ne!(painted(&run(&ctx, &host, 2.0, 0.5, vec![]), GREEN), []);
    }

    #[test]
    fn the_dot_is_narrower_than_the_arc_it_sits_on() {
        let ctx = egui::Context::default();
        let host = Host::default();

        let (left, right) = extent_of(&run(&ctx, &host, 0.0, 0.5, vec![]), DOT);

        let width = right - left;
        assert!(width > THICKNESS / 3.0 && width < THICKNESS, "{width}");
    }

    #[test]
    fn wheel_burst_keeps_a_gesture_open_until_the_host_can_record_it() {
        let ctx = egui::Context::default();
        let host = Host::default();
        frame(&ctx, &host, 0.0, 0.5, vec![]);
        frame(&ctx, &host, 0.01, 0.5, wheel());
        assert_eq!(*host.gestures.lock().expect("gestures"), ["begin"]);
        frame(&ctx, &host, 0.07, 0.55, wheel());
        assert_eq!(*host.gestures.lock().expect("gestures"), ["begin"]);
        frame(&ctx, &host, 0.3, 0.6, vec![]);
        assert_eq!(*host.gestures.lock().expect("gestures"), ["begin", "end"]);
        let requests = host.requests.lock().expect("requests");
        assert_eq!(requests.len(), 2);
        assert_close(requests[0], 0.55);
        assert_close(requests[1], 0.6);
    }

    #[test]
    fn closing_the_window_ends_a_pending_wheel_gesture_once() {
        let ctx = egui::Context::default();
        let host = Host::default();
        frame(&ctx, &host, 0.0, 0.5, vec![]);
        frame(&ctx, &host, 0.01, 0.5, wheel());
        assert_eq!(*host.gestures.lock().expect("gestures"), ["begin"]);
        drop(ctx);
        assert_eq!(*host.gestures.lock().expect("gestures"), ["begin", "end"]);
    }

    #[test]
    fn ending_a_wheel_gesture_disarms_its_window_close_cleanup() {
        let ctx = egui::Context::default();
        let host = Host::default();
        frame(&ctx, &host, 0.0, 0.5, vec![]);
        frame(&ctx, &host, 0.01, 0.5, wheel());
        frame(&ctx, &host, 0.3, 0.55, vec![]);
        drop(ctx);
        assert_eq!(*host.gestures.lock().expect("gestures"), ["begin", "end"]);
    }

    #[test]
    fn the_painted_arc_returns_with_touch_automation() {
        let ctx = egui::Context::default();
        let host = Host::default();
        let original = frame(&ctx, &host, 0.0, 0.5, vec![]);
        let changed = frame(&ctx, &host, 0.01, 0.5, wheel());
        assert!(changed > original + 10.0);
        frame(&ctx, &host, 0.02, 0.55, vec![]);
        assert_close(frame(&ctx, &host, 0.03, 0.5, vec![]), original);
    }

    #[test]
    fn rapid_wheel_notches_accumulate_but_a_refused_change_does_not_stick() {
        let ctx = egui::Context::default();
        let host = Host::default();
        let original = frame(&ctx, &host, 0.0, 0.5, vec![]);
        frame(&ctx, &host, 0.01, 0.5, wheel());
        frame(&ctx, &host, 0.02, 0.5, wheel());
        let requests = host.requests.lock().expect("requests");
        assert_eq!(requests.len(), 2);
        for (&actual, expected) in requests.iter().zip([0.55, 0.6]) {
            assert_close(actual, expected);
        }
        drop(requests);
        assert_close(frame(&ctx, &host, 0.2, 0.5, vec![]), original);
    }

    #[test]
    fn a_drag_keeps_pending_input_until_release_then_settles() {
        let ctx = egui::Context::default();
        let host = Host::default();
        let original = frame(&ctx, &host, 0.0, 0.5, vec![]);
        let button = |pressed, pos| Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let start = egui::pos2(100.0, 100.0);
        let end = egui::pos2(100.0, 80.0);
        frame(
            &ctx,
            &host,
            0.01,
            0.5,
            vec![Event::PointerMoved(start), button(true, start)],
        );
        let dragged = frame(&ctx, &host, 0.02, 0.5, vec![Event::PointerMoved(end)]);
        assert!(dragged > original + 10.0);
        assert_close(frame(&ctx, &host, 1.0, 0.5, vec![]), dragged);
        assert_close(
            frame(&ctx, &host, 1.01, 0.5, vec![button(false, end)]),
            dragged,
        );
        assert_close(frame(&ctx, &host, 1.2, 0.5, vec![]), original);
    }

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
        let mut sent = Some(Sent {
            value: 0.6,
            over: 0.5,
            at: 0.0,
        });

        // The host hasn't taken the change yet.
        assert_close(Sent::building_on(&mut sent, 0.5, 0.0, false), 0.6);
        // It has.
        assert_close(Sent::building_on(&mut sent, 0.6, 0.01, false), 0.6);
        // Automation has moved the parameter since.
        assert_close(Sent::building_on(&mut sent, 0.2, 0.02, false), 0.2);
        // Nothing sent yet.
        assert_close(Sent::building_on(&mut None, 0.3, 0.0, false), 0.3);
    }

    #[test]
    fn automation_returning_to_the_original_value_does_not_revive_a_sent_value() {
        let mut sent = Some(Sent {
            value: 0.55,
            over: 0.5,
            at: 0.0,
        });
        assert_close(Sent::building_on(&mut sent, 0.5, 0.0, false), 0.55);
        assert_close(Sent::building_on(&mut sent, 0.55, 0.01, false), 0.55);
        assert_close(Sent::building_on(&mut sent, 0.5, 0.02, false), 0.5);
    }

    #[test]
    fn a_change_the_host_never_reports_expires_after_input_ends() {
        let mut sent = Some(Sent {
            value: 0.55,
            over: 0.5,
            at: 0.0,
        });
        assert_close(Sent::building_on(&mut sent, 0.5, 0.05, false), 0.55);
        assert_close(Sent::building_on(&mut sent, 0.5, 0.1, false), 0.5);
        assert_close(Sent::building_on(&mut sent, 0.5, 0.2, false), 0.5);
    }

    #[test]
    fn a_pending_change_is_kept_during_a_long_drag() {
        let mut sent = Some(Sent {
            value: 0.55,
            over: 0.5,
            at: 0.0,
        });
        assert_close(Sent::building_on(&mut sent, 0.5, 10.0, true), 0.55);
        // Even during a drag, a value observed from the host retires the pending change.
        assert_close(Sent::building_on(&mut sent, 0.55, 10.01, true), 0.55);
        assert_close(Sent::building_on(&mut sent, 0.5, 10.02, true), 0.5);
    }

    #[test]
    fn the_arc_runs_270_degrees_clockwise_from_the_bottom_left() {
        assert_close(angle(0.0).to_degrees(), 135.0);
        assert_close(angle(0.5).to_degrees(), 270.0);
        assert_close(angle(1.0).to_degrees(), 405.0);
    }
}
