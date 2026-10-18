//! The typeface: Archivo (SIL Open Font License, `assets/fonts/OFL.txt`), bundled.
//!
//! At its normal width, in two weights. These copies have Archivo's tabular figures made the
//! default ones, so numbers keep their width while a dial moves; they're renamed "Archivo
//! Tabular" for that change. egui only draws the fonts it's given, so an installed Archivo can't
//! stand in for them.

use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily, FontId};

/// Values and labels.
const REGULAR: &[u8] = include_bytes!("../assets/fonts/ArchivoTabular-Regular.ttf");

/// Names.
const SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/ArchivoTabular-SemiBold.ttf");

/// The named font family that holds the semi-bold weight. The regular weight is egui's
/// proportional family.
pub const SEMIBOLD_FAMILY: &str = "Archivo Tabular SemiBold";

/// A weight of the typeface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weight {
    /// For values and labels.
    Regular,
    /// For names.
    SemiBold,
}

/// Makes the typeface egui's proportional font family in its regular weight, and adds the
/// semi-bold weight as [`SEMIBOLD_FAMILY`]. Call it from the editor's `build`, since each window
/// has its own egui context.
pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    for (name, bytes) in [("archivo-regular", REGULAR), ("archivo-semibold", SEMIBOLD)] {
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    }
    // egui's own fonts stay behind Archivo, for any character it lacks.
    let fallbacks = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let family = |first: &str| {
        let mut family = vec![first.to_owned()];
        family.extend(fallbacks.iter().cloned());
        family
    };
    fonts
        .families
        .insert(FontFamily::Proportional, family("archivo-regular"));
    fonts.families.insert(
        FontFamily::Name(SEMIBOLD_FAMILY.into()),
        family("archivo-semibold"),
    );
    ctx.set_fonts(fonts);
}

/// The typeface at `size` points in `weight`.
#[must_use]
pub fn font(size: f32, weight: Weight) -> FontId {
    let family = match weight {
        Weight::Regular => FontFamily::Proportional,
        Weight::SemiBold => FontFamily::Name(SEMIBOLD_FAMILY.into()),
    };
    FontId::new(size, family)
}
