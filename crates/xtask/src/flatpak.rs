//! `--flatpak`: builds for a host installed from Flathub, such as Reaper.
//!
//! A Linux build only loads in a host whose glibc is at least as new as the one it was linked
//! against. Flathub's Reaper runs on the freedesktop 25.08 runtime (glibc 2.42), so a build from a
//! newer distro can fail to load: Reaper scans it, lists nothing and shows no error. So the build
//! runs inside that runtime's SDK, with the developer's own Rust toolchain, and a build that still
//! needs a newer glibc is refused before it's installed.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, Result, bail, ensure};

/// The SDK of the runtime Flathub's Reaper runs on.
const SDK: &str = "org.freedesktop.Sdk//25.08";

/// The glibc in that runtime: the newest a build may need.
const RUNTIME_GLIBC: (u32, u32) = (2, 42);

/// Where under the workspace's `target/` the SDK's builds go, apart from native ones.
pub const TARGET_SUBDIR: &str = "flatpak-25.08";

/// Builds and bundles inside the SDK with `cargo_args`, then checks the bundle's glibc. Returns
/// the folder the bundles are in.
pub fn bundle(root: &Path, name: &str, cargo_args: &[String]) -> Result<PathBuf> {
    let target_dir = target_dir(root, cargo_args);
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

/// Use the e2e runner's isolated folder when supplied, otherwise the normal SDK build folder.
fn target_dir(root: &Path, cargo_args: &[String]) -> PathBuf {
    crate::cargo_option(cargo_args, "--target-dir")
        .map_or_else(|| root.join("target").join(TARGET_SUBDIR), PathBuf::from)
}

/// The plugin folders a Linux host scans in the user's home: `~/.vst3` and `~/.clap`.
pub fn plugin_folders() -> Result<[PathBuf; 2]> {
    folders_in_home(std::env::var_os("HOME"))
}

/// Those folders in `home`, the value of `HOME`.
fn folders_in_home(home: Option<OsString>) -> Result<[PathBuf; 2]> {
    let home = home
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
    refuse_newer_glibc(
        library,
        newest_glibc(&String::from_utf8_lossy(&output.stdout)),
    )
}

/// Fails if `needed`, the glibc the library at `library` needs, is newer than the runtime's.
fn refuse_newer_glibc(library: &Path, needed: Option<(u32, u32)>) -> Result<()> {
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
    fn test_bundles_are_separate_from_installable_bundles() {
        let root = Path::new("/plugin");
        let normal = target_dir(root, &[]);
        let e2e = root.join("target/e2e").join(TARGET_SUBDIR);
        let args = vec![
            "--target-dir".to_owned(),
            e2e.to_string_lossy().into_owned(),
        ];
        assert_eq!(normal, root.join("target").join(TARGET_SUBDIR));
        assert_eq!(target_dir(root, &args), e2e);
        assert_ne!(normal, e2e);
    }

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
    fn a_build_may_need_the_runtimes_glibc_but_nothing_newer() {
        let library = Path::new("Fake.clap");
        let (major, minor) = RUNTIME_GLIBC;

        refuse_newer_glibc(library, None).unwrap();
        refuse_newer_glibc(library, Some((major, minor - 1))).unwrap();
        refuse_newer_glibc(library, Some(RUNTIME_GLIBC)).unwrap();
        let error = refuse_newer_glibc(library, Some((major, minor + 1))).unwrap_err();
        assert!(error.to_string().contains("needs glibc"), "{error}");
    }

    #[test]
    fn a_library_that_cant_be_read_is_refused() {
        let dir = tempfile::tempdir().unwrap();

        check_glibc(&dir.path().join("missing.clap")).unwrap_err();
    }

    #[test]
    fn the_plugin_folders_are_in_home_which_must_be_set() {
        assert_eq!(
            folders_in_home(Some("/home/me".into())).unwrap(),
            [PathBuf::from("/home/me/.vst3"), "/home/me/.clap".into()]
        );
        folders_in_home(None).unwrap_err();
        folders_in_home(Some(OsString::new())).unwrap_err();
    }

    #[test]
    fn arguments_are_quoted_for_bash() {
        assert_eq!(quoted("--release"), "'--release'");
        assert_eq!(quoted("it's"), r"'it'\''s'");
    }
}
