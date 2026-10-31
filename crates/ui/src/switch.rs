//! The switch for an on/off parameter.
//!
//! A pill-shaped track with a knob that slides to the right when on, followed by the parameter's
//! name. Clicking anywhere on it, name included, flips it inside one begin/end gesture, and it
//! follows host automation.
//!
//! Off, the track is [`TRACK`] and the knob [`MUTED`]; on, the track is the plugin's colour and the
//! knob [`PANEL`].

use egui::{Align2, Id, Pos2, Rect, Response, Sense, Ui, pos2, vec2};
use nice_plug::prelude::{Param, ParamSetter};

use crate::Colour;
use crate::night::{
    INK, MUTED, PANEL, SWITCH_HEIGHT, SWITCH_NAME_OFFSET, SWITCH_TRACK_ASPECT, TEXT_SIZE, TRACK,
};
use crate::text::{Weight, font};

/// The gap between the knob and the track's edge, as a fraction of the track's height.
const KNOB_INSET: f32 = 0.125;

/// A switch for one on/off parameter, drawn and handled by [`Switch::show`].
#[must_use = "a switch does nothing until it's shown"]
pub struct Switch<'a, P: Param> {
    param: &'a P,
    setter: &'a ParamSetter<'a>,
    colour: Colour,
}

impl<P: Param> std::fmt::Debug for Switch<'_, P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Switch")
            .field("param", &self.param.name())
            .finish_non_exhaustive()
    }
}

impl<'a, P: Param> Switch<'a, P> {
    /// A switch for `param`, which it sets through `setter`, in `colour` when on.
    pub const fn new(param: &'a P, setter: &'a ParamSetter<'a>, colour: Colour) -> Self {
        Self {
            param,
            setter,
            colour,
        }
    }

    /// Draws the switch with its top left corner at `at` and the parameter's name after it, and
    /// flips it when clicked.
    #[expect(
        clippy::must_use_candidate,
        reason = "like egui's own widgets, most callers have no use for the response"
    )]
    pub fn show(self, ui: &Ui, at: Pos2) -> Response {
        let painter = ui.painter().clone();
        let track =
            Rect::from_min_size(at, vec2(SWITCH_HEIGHT * SWITCH_TRACK_ASPECT, SWITCH_HEIGHT));
        let name = painter.text(
            pos2(at.x + SWITCH_NAME_OFFSET, track.center().y),
            Align2::LEFT_CENTER,
            self.param.name(),
            font(TEXT_SIZE, Weight::SemiBold),
            INK,
        );
        let id = Id::new(("family-ui switch", self.param.name()));
        let response = ui.interact(track.union(name), id, Sense::click());
        // A second quick click is a double click to egui, and flips it back again like any other.
        if response.clicked() || response.double_clicked() {
            let on = self.param.unmodulated_normalized_value() >= 0.5;
            self.setter.begin_set_parameter(self.param);
            self.setter
                .set_parameter_normalized(self.param, if on { 0.0 } else { 1.0 });
            self.setter.end_set_parameter(self.param);
        }

        let on = self.param.unmodulated_normalized_value() >= 0.5;
        let (track_colour, knob_colour) = if on {
            (self.colour.signal, PANEL)
        } else {
            (TRACK, MUTED)
        };
        painter.rect_filled(track, SWITCH_HEIGHT / 2.0, track_colour);
        let inset = SWITCH_HEIGHT * KNOB_INSET;
        let radius = SWITCH_HEIGHT / 2.0 - inset;
        let knob_x = if on {
            track.right() - inset - radius
        } else {
            track.left() + inset + radius
        };
        painter.circle_filled(pos2(knob_x, track.center().y), radius, knob_colour);
        response
    }
}

#[cfg(test)]
mod tests {
    use egui::{Color32, Event, FullOutput, PointerButton, RawInput, Shape, Vec2};
    use nice_plug::prelude::BoolParam;

    use super::*;
    use crate::fake_host::Host;
    use crate::text::install;

    const AT: Pos2 = pos2(10.0, 10.0);
    const COLOUR: Colour = Colour {
        signal: Color32::GREEN,
        lit: Color32::GREEN,
    };

    fn context() -> egui::Context {
        let ctx = egui::Context::default();
        install(&ctx);
        ctx
    }

    /// One frame of a switch at [`AT`], with its parameter `on`.
    fn frame(
        ctx: &egui::Context,
        host: &Host,
        time: f64,
        on: bool,
        events: Vec<Event>,
    ) -> FullOutput {
        let param = BoolParam::new("Bypass", on);
        let setter = ParamSetter { raw_context: host };
        let mut output = ctx.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 100.0))),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ui| {
                Switch::new(&param, &setter, COLOUR).show(ui, AT);
            },
        );
        output.textures_delta.clear();
        output
    }

    fn click(ctx: &egui::Context, host: &Host, time: f64, on: bool, at: Pos2) {
        let button = |pressed| Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame(
            ctx,
            host,
            time,
            on,
            vec![Event::PointerMoved(at), button(true)],
        );
        frame(ctx, host, time + 0.01, on, vec![button(false)]);
    }

    /// A point on the track.
    fn track_centre() -> Pos2 {
        AT + vec2(SWITCH_HEIGHT * SWITCH_TRACK_ASPECT, SWITCH_HEIGHT) / 2.0
    }

    #[test]
    fn clicking_flips_it_inside_one_gesture() {
        for (on, flipped) in [(false, 1.0), (true, 0.0)] {
            let ctx = context();
            let host = Host::default();
            frame(&ctx, &host, 0.0, on, vec![]);

            click(&ctx, &host, 0.01, on, track_centre());

            assert_eq!(*host.requests.lock().expect("requests"), [flipped]);
            assert_eq!(*host.gestures.lock().expect("gestures"), ["begin", "end"]);
        }
    }

    #[test]
    fn a_quick_second_click_flips_it_back() {
        let ctx = context();
        let host = Host::default();
        frame(&ctx, &host, 0.0, false, vec![]);

        click(&ctx, &host, 0.01, false, track_centre());
        click(&ctx, &host, 0.05, true, track_centre());

        assert_eq!(*host.requests.lock().expect("requests"), [1.0, 0.0]);
    }

    #[test]
    fn clicking_its_name_flips_it_and_clicking_past_it_does_not() {
        let ctx = context();
        let host = Host::default();
        frame(&ctx, &host, 0.0, false, vec![]);

        click(
            &ctx,
            &host,
            0.01,
            false,
            AT + vec2(SWITCH_NAME_OFFSET + 5.0, 8.0),
        );
        assert_eq!(host.requests.lock().expect("requests").len(), 1);

        click(&ctx, &host, 1.0, false, AT + vec2(300.0, 8.0));
        assert_eq!(host.requests.lock().expect("requests").len(), 1);
    }

    struct Drawn {
        track: Rect,
        track_fill: Color32,
        corner: f32,
        knob: Pos2,
        knob_radius: f32,
        knob_fill: Color32,
        name_x: f32,
    }

    fn drawn(on: bool) -> Drawn {
        let ctx = context();
        let output = frame(&ctx, &Host::default(), 0.0, on, vec![]);
        let shapes = || output.shapes.iter().map(|shape| &shape.shape);
        let rect = shapes()
            .find_map(|shape| match shape {
                Shape::Rect(rect) => Some(rect),
                _ => None,
            })
            .expect("a track");
        let circle = shapes()
            .find_map(|shape| match shape {
                Shape::Circle(circle) => Some(circle),
                _ => None,
            })
            .expect("a knob");
        let name_x = shapes()
            .find_map(|shape| match shape {
                Shape::Text(text) => Some(text.pos.x),
                _ => None,
            })
            .expect("a name");
        Drawn {
            track: rect.rect,
            track_fill: rect.fill,
            corner: f32::from(rect.corner_radius.nw),
            knob: circle.center,
            knob_radius: circle.radius,
            knob_fill: circle.fill,
            name_x,
        }
    }

    #[test]
    fn the_knob_sits_inside_a_pill_at_the_end_for_its_state_in_that_states_colours() {
        let (off, on) = (drawn(false), drawn(true));

        for switch in [&off, &on] {
            assert_eq!(switch.track.min, AT);
            assert!((switch.track.height() - SWITCH_HEIGHT).abs() < 1e-3);
            assert!((switch.track.width() / SWITCH_HEIGHT - SWITCH_TRACK_ASPECT).abs() < 1e-3);
            assert!((switch.corner - SWITCH_HEIGHT / 2.0).abs() < 1e-3);
            assert!((switch.knob.y - switch.track.center().y).abs() < 1e-3);
            let knob = Rect::from_center_size(switch.knob, Vec2::splat(switch.knob_radius * 2.0));
            assert!(switch.track.shrink(1.0).contains_rect(knob));
            assert!(switch.knob_radius > SWITCH_HEIGHT / 4.0);
        }
        // The same gap to the edge at either end.
        let left_gap = off.knob.x - off.track.left();
        let right_gap = on.track.right() - on.knob.x;
        assert!((left_gap - right_gap).abs() < 1e-3);
        assert!(off.knob.x < off.track.center().x && on.knob.x > on.track.center().x);

        assert_eq!((off.track_fill, off.knob_fill), (TRACK, MUTED));
        assert_eq!((on.track_fill, on.knob_fill), (COLOUR.signal, PANEL));
    }

    #[test]
    fn the_name_starts_after_the_track_at_its_offset() {
        let switch = drawn(false);

        assert!((switch.name_x - (AT.x + SWITCH_NAME_OFFSET)).abs() < 1e-3);
        assert!(switch.name_x > switch.track.right());
    }
}
