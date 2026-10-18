//! `--flatpak`: builds for a host installed from Flathub, such as Reaper.
//!
//! A Linux build only loads in a host whose glibc is at least as new as the one it was linked
//! against. Flathub's Reaper runs on the freedesktop 25.08 runtime (glibc 2.42), so a build from a
//! newer distro can fail to load: Reaper scans it, lists nothing and shows no error. So the build
//! runs inside that runtime's SDK, with the developer's own Rust toolchain, and a build that still
//! needs a newer glibc is refused before it's installed.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, Result, bail, ensure};

/// The SDK of the runtime Flathub's Reaper runs on.
const SDK: &str = "org.freedesktop.Sdk//25.08";

/// The glibc in that runtime: the newest a build may need.
const RUNTIME_GLIBC: (u32, u32) = (2, 42);

/// Where under the workspace's `target/` the SDK's builds go, apart from native ones.
const TARGET_SUBDIR: &str = "flatpak-25.08";

/// Builds and bundles inside the SDK with `cargo_args`, then checks the bundle's glibc. Returns
/// the folder the bundles are in.
pub fn bundle(root: &Path, name: &str, cargo_args: &[String]) -> Result<PathBuf> {
    let target_dir = root.join("target").join(TARGET_SUBDIR);
    let mut script = format!(
        "export PATH=\"$HOME/.cargo/bin:$PATH\" CARGO_TARGET_DIR={}; cd {} && cargo xtask bundle",
        quoted(&target_dir.to_string_lossy()),
        quoted(&root.to_string_lossy()),
    );
    for arg in cargo_args {
        script.push(' ');
        script.push_str(&quoted(arg));
    }
    let status = Command::new("flatpak")
        .args(["run", "--filesystem=home", "--share=network"])
        .arg(format!("--filesystem={}", root.display()))
        .args(["--command=bash", SDK, "-c", &script])
        .status()
        .with_context(|| format!("could not run flatpak (is {SDK} installed?)"))?;
    ensure!(status.success(), "building inside {SDK} failed");

    let bundled = target_dir.join("bundled");
    check_glibc(&bundled.join(format!("{name}.clap")))?;
    Ok(bundled)
}

/// The plugin folders a Linux host scans in the user's home: `~/.vst3` and `~/.clap`.
pub fn plugin_folders() -> Result<[PathBuf; 2]> {
    let home = std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .context("HOME is not set")?;
    let home = PathBuf::from(home);
    Ok([home.join(".vst3"), home.join(".clap")])
}

/// Fails if the library at `library` needs a newer glibc than the runtime has.
fn check_glibc(library: &Path) -> Result<()> {
    let output = Command::new("objdump")
        .arg("-T")
        .arg(library)
        .output()
        .context("could not run objdump to check the build's glibc (is binutils installed?)")?;
    ensure!(
        output.status.success(),
        "objdump could not read {}",
        library.display()
    );
    let needed = newest_glibc(&String::from_utf8_lossy(&output.stdout));
    if let Some(needed) = needed
        && needed > RUNTIME_GLIBC
    {
        bail!(
            "{} needs glibc {}.{}, but {SDK} has {}.{}: the host would skip it without a word",
            library.display(),
            needed.0,
            needed.1,
            RUNTIME_GLIBC.0,
            RUNTIME_GLIBC.1,
        );
    }
    Ok(())
}

/// The newest `GLIBC_<major>.<minor>` symbol version in `objdump -T` output.
fn newest_glibc(symbols: &str) -> Option<(u32, u32)> {
    symbols
        .split("GLIBC_")
        .skip(1)
        .filter_map(|rest| {
            let mut parts = rest.split('.');
            let major = parts.next()?.parse().ok()?;
            let minor: String = parts
                .next()?
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            Some((major, minor.parse().ok()?))
        })
        .max()
}

/// `text` quoted for bash.
fn quoted(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_newest_glibc_is_found_among_the_symbol_versions() {
        let symbols = "\
0000000000000000      DF *UND*  0000000000000000 (GLIBC_2.2.5) memcpy
0000000000000000      DF *UND*  0000000000000000 (GLIBC_2.43) acosf
0000000000000000      DF *UND*  0000000000000000 (GLIBC_2.9) pipe2
0000000000000000  w   D  *UND*  0000000000000000  __gmon_start__";

        assert_eq!(newest_glibc(symbols), Some((2, 43)));
    }

    #[test]
    fn a_library_without_glibc_symbols_needs_none() {
        assert_eq!(newest_glibc("no versions here"), None);
    }

    #[test]
    fn arguments_are_quoted_for_bash() {
        assert_eq!(quoted("--release"), "'--release'");
        assert_eq!(quoted("it's"), r"'it'\''s'");
    }
}
