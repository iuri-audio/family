//! Night's colours and sizes, shared by every plugin (the tokens in `docs/family-identity.md`).
//!
//! Sizes are in points: logical pixels on a plugin's design-size canvas, which [`Canvas`] scales
//! evenly.
//!
//! [`Canvas`]: crate::Canvas

use egui::Color32;

use crate::glow::Glow;

/// The panel.
pub const PANEL: Color32 = Color32::from_rgb(0x17, 0x1b, 0x20);

/// Names and values.
pub const INK: Color32 = Color32::from_rgb(0xe9, 0xed, 0xf0);

/// Units, end labels and secondary values, the switch's knob when off, and the corner grip.
pub const MUTED: Color32 = Color32::from_rgb(0x86, 0x91, 0x9c);

/// The rule between areas.
pub const RULE: Color32 = Color32::from_rgb(0x26, 0x2c, 0x33);

/// A dial's track, and the switch's track when off.
pub const TRACK: Color32 = Color32::from_rgb(0x26, 0x2d, 0x35);

/// The lamp when dark.
pub const LAMP_DARK: Color32 = Color32::from_rgb(0x2e, 0x35, 0x3d);

/// The dot at the end of a dial's arc.
pub const DOT: Color32 = Color32::WHITE;

/// The default text size, for names.
pub const TEXT_SIZE: f32 = 15.0;

/// The size of secondary values, units and end labels.
pub const SMALL_TEXT_SIZE: f32 = 13.0;

/// The height of the bar in the plugin's colour along the top edge.
pub const COLOUR_BAR_HEIGHT: f32 = 3.0;

/// The width of the rule between areas.
pub const RULE_WIDTH: f32 = 1.0;

/// The lamp's radius.
pub const LAMP_RADIUS: f32 = 4.0;

/// The switch's height; its track is [`SWITCH_TRACK_ASPECT`] times as wide.
pub const SWITCH_HEIGHT: f32 = 16.0;

/// The switch track's width as a multiple of its height.
pub const SWITCH_TRACK_ASPECT: f32 = 1.875;

/// Where the switch's name starts, from the switch's left edge.
pub const SWITCH_NAME_OFFSET: f32 = 40.0;

/// The glow on a lit dial's arc: the signal colour at 55 % opacity (`8c`).
///
/// Its width was chosen by eye against the vizia build's `box-shadow: 0 0 18px`, which femtovg
/// rendered at a sigma of about a quarter of the blur radius, not the half its code asks for.
pub const DIAL_GLOW: Glow = Glow {
    sigma: 4.5,
    spread: 0.0,
    opacity: 140.0 / 255.0,
};

/// The glow on a lit lamp: the signal colour at 80 % opacity (`cc`), with the lamp grown by 3
/// points before it's blurred (the vizia build's `box-shadow: 0 0 10px 3px`).
pub const LAMP_GLOW: Glow = Glow {
    sigma: 2.5,
    spread: 3.0,
    opacity: 204.0 / 255.0,
};
