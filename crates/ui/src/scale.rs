//! The user's editor scale, which the plugin saves with the project.
//!
//! nice-plug doesn't save an editor's size, so a plugin keeps an [`EditorScale`] as a persistent
//! field of its parameters, under the key its vizia build saved its `ViziaState` in. It's saved in
//! the same form, `{"scale_factor": 1.5}`, so projects saved with that build reopen at their
//! scale.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use nice_plug::params::persist::PersistentField;
use serde::{Deserialize, Serialize};

/// The smallest scale the corner grip goes to.
pub const MIN: f32 = 0.5;

/// The largest scale the corner grip goes to.
pub const MAX: f32 = 3.0;

/// The user's scale of the editor, on top of the host's display scale: 1 is the design size.
#[derive(Debug)]
pub struct EditorScale(AtomicU32);

impl EditorScale {
    /// A scale of `scale`, clamped to the grip's range.
    #[must_use]
    pub const fn new(scale: f32) -> Self {
        Self(AtomicU32::new(clamped(scale).to_bits()))
    }

    /// The scale.
    #[must_use]
    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    /// Sets the scale to `scale`, clamped to the grip's range.
    pub fn set(&self, scale: f32) {
        self.0.store(clamped(scale).to_bits(), Ordering::Relaxed);
    }
}

impl Default for EditorScale {
    fn default() -> Self {
        Self::new(1.0)
    }
}

/// `scale` within the grip's range, or 1 if it isn't a number.
const fn clamped(scale: f32) -> f32 {
    if scale.is_nan() {
        1.0
    } else {
        scale.clamp(MIN, MAX)
    }
}

/// An [`EditorScale`] as it's saved: the form vizia's `ViziaState` was saved in.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Saved {
    /// The user's scale.
    scale_factor: f64,
}

impl PersistentField<'_, Saved> for EditorScale {
    fn set(&self, saved: Saved) {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a scale is clamped to a small range, where f32 is precise enough"
        )]
        Self::set(self, saved.scale_factor as f32);
    }

    fn map<F, R>(&self, f: F) -> R
    where
        F: Fn(&Saved) -> R,
    {
        f(&Saved {
            scale_factor: f64::from(self.get()),
        })
    }
}

/// The form a plugin keeps it in, shared with its editor.
impl PersistentField<'_, Saved> for Arc<EditorScale> {
    fn set(&self, saved: Saved) {
        PersistentField::set(self.as_ref(), saved);
    }

    fn map<F, R>(&self, f: F) -> R
    where
        F: Fn(&Saved) -> R,
    {
        self.as_ref().map(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scale_saved_by_the_vizia_build_is_read_back() {
        let scale = EditorScale::default();

        // A vizia build's `editor-state` field, as nih-plug saved it.
        let saved: Saved = serde_json::from_str(r#"{"scale_factor":1.5}"#).unwrap();
        PersistentField::set(&scale, saved);

        assert!((scale.get() - 1.5).abs() < f32::EPSILON);
    }

    #[test]
    fn a_scale_is_saved_in_the_vizia_builds_form() {
        let scale = EditorScale::new(1.25);

        let saved = scale.map(|saved| serde_json::to_string(saved).unwrap());

        assert_eq!(saved, r#"{"scale_factor":1.25}"#);
    }

    #[test]
    fn a_scale_out_of_range_is_clamped_and_one_that_is_not_a_number_is_1() {
        assert!((EditorScale::new(10.0).get() - MAX).abs() < f32::EPSILON);
        assert!((EditorScale::new(0.1).get() - MIN).abs() < f32::EPSILON);
        assert!((EditorScale::new(f32::NAN).get() - 1.0).abs() < f32::EPSILON);
    }
}
