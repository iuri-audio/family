//! What each plugin's `render` shares beyond the WAV commands: loading its tuning.

use std::path::Path;

use anyhow::{Context as _, Result};
use serde::de::DeserializeOwned;

/// Loads a plugin's tuning from `path` if given, else from `tuning.toml` in the working directory
/// if it exists, else uses the compiled-in defaults.
///
/// Keys a file leaves out take their defaults when the tuning says so with `#[serde(default)]`.
///
/// # Errors
///
/// If the file can't be read, or isn't valid tuning.
pub fn find_tuning<T: DeserializeOwned + Default>(path: Option<&Path>) -> Result<T> {
    let in_working_dir = Path::new("tuning.toml");
    match path {
        Some(path) => load_tuning(path),
        None if in_working_dir.exists() => load_tuning(in_working_dir),
        None => Ok(T::default()),
    }
}

/// Loads tuning from a toml file.
fn load_tuning<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("can't read {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("invalid tuning in {}", path.display()))
}
