//! `cargo xtask install`: copies the bundles into a plugin folder without leaving a broken install
//! behind.
//!
//! A host that has the plugin loaded keeps its library open, and on WSL writing over it fails
//! with "Permission denied". So every file the install would replace is first checked for
//! writing, and only if they all pass are the new files copied in, each to a temporary name
//! beside its target. Then each target is moved aside and its new file renamed into place; if a
//! rename still fails, the moved-aside files are put back. When a check fails, nothing has been
//! touched.

use std::ffi::OsString;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};

/// The plugin folder from `value`, the value of the environment variable `variable`, or an error
/// naming the variable.
pub fn plugin_dir(value: Option<OsString>, variable: &str) -> Result<PathBuf> {
    match value {
        Some(value) if !value.is_empty() => Ok(PathBuf::from(value)),
        _ => bail!(
            "{variable} is not set: set it to the folder that holds the VST3 and CLAP folders, \
             e.g. {variable}=/mnt/c/Users/<you>/Plugins"
        ),
    }
}

/// The VST3 and CLAP folders under the plugin folder `plugin_dir`: `VST3/` and `CLAP/`.
pub fn folders_under(plugin_dir: &Path) -> [PathBuf; 2] {
    [plugin_dir.join("VST3"), plugin_dir.join("CLAP")]
}

/// Copies the bundles named `name` from `bundled` into `folders`, the VST3 folder and the CLAP
/// folder, replacing what is there. Returns the installed bundles' paths.
///
/// If a file it would replace can't be written, which is what a host holding the plugin open
/// looks like, it fails without changing anything.
pub fn install(bundled: &Path, folders: &[PathBuf; 2], name: &str) -> Result<Vec<PathBuf>> {
    let mut copies = Vec::new();
    let mut installed = Vec::new();
    for (bundle, folder) in [format!("{name}.vst3"), format!("{name}.clap")]
        .iter()
        .zip(folders)
    {
        let source = bundled.join(bundle);
        let target = folder.join(bundle);
        copy_plan(&source, &target, &mut copies)
            .with_context(|| format!("can't read the bundle {}", source.display()))?;
        installed.push(target);
    }

    for (_, target) in &copies {
        check_writable(target)?;
    }

    let mut staged = Vec::new();
    let staging = copies.iter().try_for_each(|(source, target)| {
        let temporary = temporary_path(target);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("can't create {}", parent.display()))?;
        }
        fs::copy(source, &temporary).with_context(|| {
            format!("can't copy {} to {}", source.display(), temporary.display())
        })?;
        staged.push((temporary, target));
        anyhow::Ok(())
    });
    if let Err(error) = staging {
        remove_all(staged.iter().map(|(temporary, _)| temporary));
        return Err(error);
    }

    let mut swapped: Vec<Swapped> = Vec::new();
    for (index, (temporary, target)) in staged.iter().enumerate() {
        if let Err(error) = swap_in(temporary, target, &mut swapped) {
            let restored = roll_back(&swapped);
            remove_all(staged.iter().skip(index).map(|(temporary, _)| temporary));
            return Err(locked_error(target, &error, restored));
        }
    }
    remove_all(swapped.iter().filter_map(|swap| swap.previous.as_ref()));
    Ok(installed)
}

/// A file the install has put in place, and where the file it replaced was moved to, if there was
/// one.
struct Swapped {
    target: PathBuf,
    previous: Option<PathBuf>,
}

/// Moves `target`, if there is one, aside to a backup name, then renames `temporary` to `target`,
/// recording what it did in `swapped`.
fn swap_in(temporary: &Path, target: &Path, swapped: &mut Vec<Swapped>) -> std::io::Result<()> {
    let previous = if target.exists() {
        let backup = hidden_beside(target, "previous");
        fs::rename(target, &backup)?;
        Some(backup)
    } else {
        None
    };
    let result = fs::rename(temporary, target);
    // Recorded even when the rename failed, so a roll-back puts the moved-aside file back.
    swapped.push(Swapped {
        target: target.to_owned(),
        previous,
    });
    result
}

/// Undoes the swaps, newest first: puts each replaced file back, or removes a new file that
/// replaced nothing. Returns whether everything was put back.
fn roll_back(swapped: &[Swapped]) -> bool {
    swapped.iter().rev().fold(true, |restored, swap| {
        let undone = swap.previous.as_ref().map_or_else(
            || {
                fs::remove_file(&swap.target).or_else(|error| {
                    // A new file whose rename failed was never there.
                    if error.kind() == ErrorKind::NotFound {
                        Ok(())
                    } else {
                        Err(error)
                    }
                })
            },
            |previous| fs::rename(previous, &swap.target),
        );
        restored && undone.is_ok()
    })
}

/// Lists every file under `source` (a file or a folder) with where it goes under `target`.
pub fn copy_plan(source: &Path, target: &Path, copies: &mut Vec<(PathBuf, PathBuf)>) -> Result<()> {
    if source.is_dir() {
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            copy_plan(&entry.path(), &target.join(entry.file_name()), copies)?;
        }
    } else if source.is_file() {
        copies.push((source.to_owned(), target.to_owned()));
    } else {
        bail!("{} is missing", source.display());
    }
    Ok(())
}

/// Fails if `target` exists and can't be opened for writing. It isn't changed either way.
fn check_writable(target: &Path) -> Result<()> {
    if !target.exists() {
        return Ok(());
    }
    fs::OpenOptions::new()
        .write(true)
        .open(target)
        .map(drop)
        .map_err(|error| locked_error(target, &error, true))
}

/// The error for a target that can't be replaced: on "Permission denied", most likely a host has
/// the plugin open. `intact` says whether the previous build is still fully in place.
fn locked_error(target: &Path, error: &std::io::Error, intact: bool) -> anyhow::Error {
    let state = if intact {
        "Nothing was installed; the previous build is still in place."
    } else {
        "Putting the previous build back failed too, so the install is a mix of both builds."
    };
    if error.kind() == ErrorKind::PermissionDenied {
        anyhow::anyhow!(
            "can't replace {}: permission denied. A host probably has the plugin open: close the \
             host and run it again. {state}",
            target.display()
        )
    } else {
        anyhow::anyhow!("can't replace {}: {error}. {state}", target.display())
    }
}

/// The name a new file is copied to before it replaces `target`: beside it, hidden.
fn temporary_path(target: &Path) -> PathBuf {
    hidden_beside(target, "installing")
}

/// A hidden name beside `target`: `.<name>.<suffix>`.
fn hidden_beside(target: &Path, suffix: &str) -> PathBuf {
    let mut name = OsString::from(".");
    name.push(target.file_name().unwrap_or_default());
    name.push(".");
    name.push(suffix);
    target.with_file_name(name)
}

/// Removes leftover temporary or backup files, ignoring failures: they are only leftovers.
fn remove_all<'a>(paths: impl IntoIterator<Item = &'a PathBuf>) {
    for path in paths {
        if fs::remove_file(path).is_err() {
            // Already gone, or as stuck as the target; either way, not worth failing over.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake build in `dir/bundled`: a VST3 bundle folder and a CLAP file, with `content`.
    fn bundle(dir: &Path, content: &str) -> PathBuf {
        let bundled = dir.join("bundled");
        let library = bundled.join("Fake.vst3/Contents/x86_64-win");
        fs::create_dir_all(&library).unwrap();
        fs::write(library.join("Fake.vst3"), format!("vst3 {content}")).unwrap();
        fs::write(bundled.join("Fake.clap"), format!("clap {content}")).unwrap();
        bundled
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    const INSTALLED_VST3: &str = "VST3/Fake.vst3/Contents/x86_64-win/Fake.vst3";
    const INSTALLED_CLAP: &str = "CLAP/Fake.clap";

    /// Every file under `dir`, relative to it.
    fn all_files(dir: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        copy_plan(dir, Path::new(""), &mut found).unwrap();
        let mut names: Vec<PathBuf> = found.into_iter().map(|(_, name)| name).collect();
        names.sort();
        names
    }

    #[test]
    fn installs_both_formats_into_their_folders() {
        let dir = tempfile::tempdir().unwrap();
        let bundled = bundle(dir.path(), "new");
        let plugins = dir.path().join("Plugins");

        let installed = install(&bundled, &folders_under(&plugins), "Fake").unwrap();

        assert_eq!(
            installed,
            [
                plugins.join("VST3/Fake.vst3"),
                plugins.join("CLAP/Fake.clap")
            ]
        );
        assert_eq!(read(&plugins.join(INSTALLED_VST3)), "vst3 new");
        assert_eq!(read(&plugins.join(INSTALLED_CLAP)), "clap new");
        assert_eq!(
            all_files(&plugins),
            [PathBuf::from(INSTALLED_CLAP), PathBuf::from(INSTALLED_VST3)]
        );
    }

    #[test]
    fn replaces_an_installed_build() {
        let dir = tempfile::tempdir().unwrap();
        let plugins = dir.path().join("Plugins");
        install(&bundle(dir.path(), "old"), &folders_under(&plugins), "Fake").unwrap();

        install(&bundle(dir.path(), "new"), &folders_under(&plugins), "Fake").unwrap();

        assert_eq!(read(&plugins.join(INSTALLED_VST3)), "vst3 new");
        assert_eq!(read(&plugins.join(INSTALLED_CLAP)), "clap new");
    }

    #[test]
    fn a_locked_file_fails_clearly_and_leaves_the_installed_build_intact() {
        // A read-only file stands in for a library a host holds open: both fail with "Permission
        // denied" when opened for writing.
        for locked in [INSTALLED_VST3, INSTALLED_CLAP] {
            let dir = tempfile::tempdir().unwrap();
            let plugins = dir.path().join("Plugins");
            install(&bundle(dir.path(), "old"), &folders_under(&plugins), "Fake").unwrap();
            let mut permissions = fs::metadata(plugins.join(locked)).unwrap().permissions();
            permissions.set_readonly(true);
            fs::set_permissions(plugins.join(locked), permissions).unwrap();

            let error =
                install(&bundle(dir.path(), "new"), &folders_under(&plugins), "Fake").unwrap_err();

            let message = format!("{error:#}");
            assert!(
                message.contains("close the host and run it again"),
                "{message}"
            );
            assert!(
                message.contains(locked.rsplit('/').next().unwrap()),
                "{message}"
            );
            assert_eq!(read(&plugins.join(INSTALLED_VST3)), "vst3 old");
            assert_eq!(read(&plugins.join(INSTALLED_CLAP)), "clap old");
            assert_eq!(
                all_files(&plugins),
                [PathBuf::from(INSTALLED_CLAP), PathBuf::from(INSTALLED_VST3)],
                "no leftovers"
            );
        }
    }

    #[test]
    fn a_rename_that_fails_after_the_checks_puts_the_installed_build_back() {
        let dir = tempfile::tempdir().unwrap();
        let plugins = dir.path().join("Plugins");
        install(&bundle(dir.path(), "old"), &folders_under(&plugins), "Fake").unwrap();
        // The CLAP can be written, so it passes the check, but the name it would be moved aside
        // to is taken by a folder, so moving it fails after the VST3 has already been replaced.
        let blocker = plugins.join("CLAP/.Fake.clap.previous");
        fs::create_dir_all(&blocker).unwrap();
        fs::write(blocker.join("in the way"), "").unwrap();

        let error =
            install(&bundle(dir.path(), "new"), &folders_under(&plugins), "Fake").unwrap_err();

        let message = format!("{error:#}");
        assert!(
            message.contains("previous build is still in place"),
            "{message}"
        );
        assert_eq!(read(&plugins.join(INSTALLED_VST3)), "vst3 old");
        assert_eq!(read(&plugins.join(INSTALLED_CLAP)), "clap old");
        assert_eq!(
            all_files(&plugins),
            [
                PathBuf::from("CLAP/.Fake.clap.previous/in the way"),
                PathBuf::from(INSTALLED_CLAP),
                PathBuf::from(INSTALLED_VST3)
            ],
            "no leftovers"
        );
    }

    #[test]
    fn a_missing_bundle_is_an_error_and_installs_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let bundled = bundle(dir.path(), "new");
        fs::remove_file(bundled.join("Fake.clap")).unwrap();
        let plugins = dir.path().join("Plugins");

        let error = install(&bundled, &folders_under(&plugins), "Fake").unwrap_err();

        assert!(format!("{error:#}").contains("Fake.clap"), "{error:#}");
        assert!(!plugins.exists());
    }

    #[test]
    fn the_plugin_folder_comes_from_the_environment_variable() {
        assert_eq!(
            plugin_dir(Some("/somewhere/Plugins".into()), "FAKE_PLUGIN_DIR").unwrap(),
            PathBuf::from("/somewhere/Plugins")
        );
        for unset in [None, Some(OsString::new())] {
            let error = plugin_dir(unset, "FAKE_PLUGIN_DIR").unwrap_err();
            assert!(error.to_string().contains("FAKE_PLUGIN_DIR"), "{error}");
        }
    }
}
