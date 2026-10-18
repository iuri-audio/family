//! The family's editor look, Night (see `docs/family-identity.md`), in egui on nice-plug: the
//! look's colours and sizes, the bundled typeface, the dial, the switch, the lamp and their glow,
//! and the evenly scaled [`Canvas`] a plugin lays its editor out on.
//!
//! A plugin's editor is a `nice_plug_egui::NiceEguiApp` that calls [`text::install`] and
//! [`Canvas::opened`] in `build`, then draws inside [`Canvas::show`] in `ui`, at fixed
//! coordinates on its design-size canvas. The components take the plugin's [`Colour`] and know
//! nothing else about the plugin they're in.

pub mod canvas;
pub mod dial;
pub mod glow;
pub mod lamp;
pub mod night;
pub mod scale;
pub mod switch;
pub mod text;

use egui::Color32;

pub use canvas::Canvas;
pub use dial::Dial;
pub use glow::Glow;
pub use lamp::Lamp;
pub use scale::EditorScale;
pub use switch::Switch;

/// A plugin's signal colour, which it uses for the value arc, the lamp, the switch when on and the
/// bar along the top edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Colour {
    /// The colour itself: the arc, the switch when on, the bar, and the glows at their own
    /// opacities.
    pub signal: Color32,
    /// A lighter tint, for the lit lamp.
    pub lit: Color32,
}
