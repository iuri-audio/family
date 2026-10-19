//! What each plugin's `render` shares: loading its tuning.
#![expect(
    clippy::unwrap_used,
    reason = "test helpers; a failure should fail the test"
)]

use std::path::Path;

use family_wav::render::find_tuning;

/// A stand-in for a plugin's tuning: any key a file leaves out takes its default.
#[derive(Debug, PartialEq, serde::Deserialize)]
#[serde(default)]
struct Tuning {
    lookahead_ms: f32,
    bands: u32,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            lookahead_ms: 10.0,
            bands: 4,
        }
    }
}

fn write(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
}

#[test]
fn a_given_tuning_file_is_loaded_and_missing_keys_take_their_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mine.toml");
    write(&path, "bands = 6\n");

    let tuning: Tuning = find_tuning(Some(&path)).unwrap();

    assert_eq!(
        tuning,
        Tuning {
            lookahead_ms: 10.0,
            bands: 6
        }
    );
}

#[test]
fn an_invalid_tuning_file_is_an_error_naming_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("broken.toml");
    write(&path, "bands = \"six\"\n");

    let error = find_tuning::<Tuning>(Some(&path)).unwrap_err();

    assert!(format!("{error}").contains("broken.toml"), "{error:#}");
}

#[test]
fn a_missing_tuning_file_is_an_error_naming_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("absent.toml");

    let error = find_tuning::<Tuning>(Some(&path)).unwrap_err();

    assert!(format!("{error}").contains("absent.toml"), "{error:#}");
}
