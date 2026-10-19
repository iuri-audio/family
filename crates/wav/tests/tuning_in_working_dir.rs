//! Without a tuning file given, `render` looks for `tuning.toml` in the working directory.
//!
//! Its own test binary, with one test, because it changes the process's working directory.

use family_wav::render::find_tuning;

/// A stand-in for a plugin's tuning: any key a file leaves out takes its default.
#[derive(Debug, PartialEq, serde::Deserialize)]
#[serde(default)]
struct Tuning {
    bands: u32,
}

impl Default for Tuning {
    fn default() -> Self {
        Self { bands: 4 }
    }
}

#[test]
fn tuning_toml_in_the_working_directory_is_used_if_it_exists_and_the_defaults_if_not() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();

    assert_eq!(find_tuning::<Tuning>(None).unwrap(), Tuning { bands: 4 });

    std::fs::write(dir.path().join("tuning.toml"), "bands = 6\n").unwrap();

    assert_eq!(find_tuning::<Tuning>(None).unwrap(), Tuning { bands: 6 });
}
