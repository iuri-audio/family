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
