//! Build helper for the family's plugins: builds a plugin and bundles it as VST3 and CLAP into
//! `target/bundled/`, installs the bundles, and runs the plugin's end-to-end checks in Reaper.
//!
//! Each plugin's `xtask` crate is a one-line `main` that calls [`main`] with its [`Plugin`]:
//!
//! ```text
//! cargo xtask bundle --release                                   # native (Linux)
//! cargo xtask bundle --release --target x86_64-pc-windows-msvc   # Windows, via cargo-xwin
//! cargo xtask install --release --target x86_64-pc-windows-msvc  # bundle, then install
//! cargo xtask install --flatpak --release                         # for Flathub's Reaper
//! cargo xtask e2e --release --target x86_64-pc-windows-msvc      # end-to-end checks in Reaper
//! cargo xtask e2e --release                                      # ...in a native Linux Reaper
//! ```
//!
//! Any arguments after `bundle`, `install` or `e2e` go to `cargo build`, apart from `--flatpak`. A
//! Windows MSVC target built from another OS goes through `cargo xwin build`, which supplies the
//! MSVC headers, libraries and linker.
//!
//! `install` copies `<Name>.vst3` and `<Name>.clap` into the `VST3/` and `CLAP/` folders under the
//! directory in the plugin's environment variable.
//!
//! `--flatpak` builds for a Linux host installed from Flathub, such as Reaper: inside the SDK of
//! the runtime it runs on, into `target/flatpak-25.08/`, refusing a build that needs a newer glibc
//! than the runtime has. With `install`, it installs into `~/.vst3` and `~/.clap`, so it needs no
//! environment variable.
//!
//! `e2e` bundles with the test hooks (the plugin's `e2e-hooks` feature, which forwards family-ui's)
//! into `target/e2e/`, never where a release build goes, then runs the end-to-end checks in Reaper:
//! on Windows from WSL with a Windows MSVC target, in Flathub's Reaper with `--flatpak`, or in a
//! Reaper installed natively on Linux with a native build. See
//! [`e2e`] and `docs/driving-reaper.md`.
//!
//! This wraps `nice_plug_xtask::bundle` instead of using `nice_plug_xtask::main`, because `main`
//! takes the outermost directory with a `Cargo.toml` as the workspace root, which is the wrong
//! one when a plugin's repo is checked out inside another (as git worktrees under `.claude/` are).

mod e2e;
mod flatpak;
mod install;

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context as _, bail, ensure};

/// What the build helper needs to know about one plugin.
#[derive(Debug, Clone, Copy)]
pub struct Plugin<'a> {
    /// The package that builds the plugin library, such as `doubler-plugin`.
    pub package: &'a str,
    /// The bundles' name, as the plugin's `bundler.toml` gives it: `Doubler` makes `Doubler.vst3`
    /// and `Doubler.clap`.
    pub name: &'a str,
    /// The environment variable naming the folder `install` copies into, such as
    /// `DOUBLER_PLUGIN_DIR`.
    pub install_dir_variable: &'a str,
    /// The plugin's workspace root, where the build runs: the folder above its `xtask` crate.
    pub workspace_root: &'a Path,
    /// Whether the plugin has an editor of its own, on `family-ui`. Without one, `e2e` builds
    /// without the test hooks (the plugin needs no `e2e-hooks` feature), skips the checks that
    /// need an editor, and the others leave out their editor steps.
    pub editor: bool,
}

const USAGE: &str = "usage: cargo xtask bundle|install|e2e [--flatpak] [cargo build arguments, e.g. \
                     --release --target <triple>]";

/// Runs `bundle`, `install` or `e2e` for `plugin`, with the arguments the process was started with.
///
/// # Errors
///
/// If the command is none of these, or if building, bundling or installing fails.
pub fn main(plugin: &Plugin<'_>) -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let command = args.next();
    let mut cargo_args: Vec<String> = args.collect();
    let flatpak = cargo_args.iter().any(|arg| arg == "--flatpak");
    cargo_args.retain(|arg| arg != "--flatpak");
    let bundle = |cargo_args: &[String]| {
        if flatpak {
            flatpak::bundle(plugin.workspace_root, plugin.name, cargo_args)
        } else {
            bundle(plugin, cargo_args)
        }
    };
    match command.as_deref() {
        Some("bundle") => {
            bundle(&cargo_args)?;
            Ok(())
        }
        Some("install") => {
            // Fail before the build if there is nowhere to install to.
            let folders = if flatpak {
                flatpak::plugin_folders()?
            } else {
                let variable = plugin.install_dir_variable;
                install::folders_under(&install::plugin_dir(std::env::var_os(variable), variable)?)
            };
            let bundled = bundle(&cargo_args)?;
            let installed = install::install(&bundled, &folders, plugin.name)?;
            print_installed(plugin.workspace_root, &cargo_args, &installed);
            Ok(())
        }
        Some("e2e") => {
            // Fail before the build if there is no Reaper to run the checks in.
            let host = e2e::Host::for_build(flatpak, target(&cargo_args))?;
            if plugin.editor {
                cargo_args.extend(["--features".to_owned(), e2e::HOOKS_FEATURE.to_owned()]);
            }
            let mut target_dir = plugin
                .workspace_root
                .join("target")
                .join(e2e::TARGET_SUBDIR);
            if flatpak {
                target_dir.push(flatpak::TARGET_SUBDIR);
            }
            cargo_args.extend([
                "--target-dir".to_owned(),
                target_dir.to_string_lossy().into_owned(),
            ]);
            let bundled = bundle(&cargo_args)?;
            e2e::run(plugin, &bundled, host)
        }
        _ => bail!(USAGE),
    }
}

/// Builds the plugin with `cargo_args` and bundles it. Returns the folder the bundles are in.
fn bundle(plugin: &Plugin<'_>, cargo_args: &[String]) -> anyhow::Result<PathBuf> {
    let root = plugin.workspace_root;
    std::env::set_current_dir(root).context("could not change to the workspace root")?;
    let package = plugin.package;

    let mut build = Command::new("cargo");
    if !cfg!(windows) && is_msvc(target(cargo_args)) {
        build.arg("xwin");
    }
    build.args(["build", "--package", package]).args(cargo_args);
    let status = build
        .status()
        .context("could not run cargo (is cargo-xwin installed for Windows builds?)")?;
    ensure!(status.success(), "building {package} failed");

    let target_dir = cargo_option(cargo_args, "--target-dir").map_or_else(
        || std::env::var_os("CARGO_TARGET_DIR").map_or_else(|| root.join("target"), PathBuf::from),
        PathBuf::from,
    );
    nice_plug_xtask::bundle(&target_dir, package, cargo_args, false)?;
    Ok(target_dir.join("bundled"))
}

/// Whether a target triple is a Windows MSVC one.
fn is_msvc(target: Option<&str>) -> bool {
    target.is_some_and(|target| target.ends_with("-pc-windows-msvc"))
}

/// Prints what was installed: where, and the build's profile, target and commit.
#[expect(
    clippy::print_stdout,
    reason = "reporting the install is what the command is for"
)]
fn print_installed(root: &Path, cargo_args: &[String], installed: &[PathBuf]) {
    for path in installed {
        println!("installed {}", path.display());
    }
    let target = target(cargo_args).map_or_else(host_target, str::to_owned);
    println!(
        "build: profile {}, target {target}, commit {}",
        profile(cargo_args),
        commit(root)
    );
}

/// The value of the cargo option `name`, given as `name value` or `name=value`.
fn cargo_option<'a>(cargo_args: &'a [String], name: &str) -> Option<&'a str> {
    let mut args = cargo_args.iter();
    while let Some(arg) = args.next() {
        if arg == name {
            return args.next().map(String::as_str);
        }
        if let Some(value) = arg
            .strip_prefix(name)
            .and_then(|rest| rest.strip_prefix('='))
        {
            return Some(value);
        }
    }
    None
}

/// The target triple the cargo arguments build for, if they name one.
fn target(cargo_args: &[String]) -> Option<&str> {
    cargo_option(cargo_args, "--target")
}

/// The profile the cargo arguments build with.
fn profile(cargo_args: &[String]) -> &str {
    let release = cargo_args
        .iter()
        .any(|arg| arg == "--release" || arg == "-r");
    cargo_option(cargo_args, "--profile").unwrap_or(if release { "release" } else { "debug" })
}

/// The host's target triple, from `rustc -vV`, or `host` if that fails.
fn host_target() -> String {
    run_text(Path::new("."), "rustc", &["-vV"])
        .and_then(|text| {
            text.lines()
                .find_map(|line| line.strip_prefix("host: ").map(str::to_owned))
        })
        .unwrap_or_else(|| "host".to_owned())
}

/// The commit the build is from, with `-dirty` if the working tree has uncommitted changes to
/// tracked files, or `unknown` if git can't tell.
fn commit(root: &Path) -> String {
    let Some(commit) = run_text(root, "git", &["rev-parse", "--short", "HEAD"]) else {
        return "unknown".to_owned();
    };
    let dirty = run_text(
        root,
        "git",
        &["status", "--porcelain", "--untracked-files=no"],
    )
    .is_some_and(|status| !status.is_empty());
    format!("{}{}", commit.trim(), if dirty { "-dirty" } else { "" })
}

/// The standard output of a command run in `dir`, if it succeeds.
fn run_text(dir: &Path, program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program)
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn args(args: &[&str]) -> Vec<String> {
        args.iter().map(|&arg| arg.to_owned()).collect()
    }

    #[test]
    fn the_profile_comes_from_the_cargo_arguments() {
        assert_eq!(profile(&args(&[])), "debug");
        assert_eq!(profile(&args(&["--release"])), "release");
        assert_eq!(profile(&args(&["-r"])), "release");
        assert_eq!(profile(&args(&["--profile", "bench"])), "bench");
        assert_eq!(profile(&args(&["--profile=bench"])), "bench");
    }

    #[test]
    fn a_commands_output_is_its_trimmed_standard_output_if_it_succeeds() {
        let here = Path::new(".");

        assert_eq!(
            run_text(here, "sh", &["-c", "echo ' hi '"]),
            Some("hi".to_owned())
        );
        assert_eq!(run_text(here, "sh", &["-c", "echo hi; exit 1"]), None);
        assert_eq!(run_text(here, "no-such-program", &[]), None);
    }

    #[test]
    fn the_commands_directory_is_the_one_asked_for() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("marker"), "").unwrap();

        assert_eq!(run_text(dir.path(), "ls", &[]), Some("marker".to_owned()));
    }

    #[test]
    fn the_host_target_is_rustcs() {
        let host = host_target();

        assert!(host.split('-').count() >= 3, "{host}");
    }

    #[test]
    fn the_commit_is_marked_dirty_only_with_uncommitted_changes_to_tracked_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        assert_eq!(commit(root), "unknown");

        let git = |args: &[&str]| {
            let status = Command::new("git")
                .args(["-c", "user.name=test", "-c", "user.email=test@example.com"])
                .args(args)
                .current_dir(root)
                .output()
                .unwrap()
                .status;
            assert!(status.success(), "git {args:?}");
        };
        git(&["init"]);
        fs::write(root.join("tracked"), "one").unwrap();
        git(&["add", "tracked"]);
        git(&["commit", "-m", "first"]);
        let clean = run_text(root, "git", &["rev-parse", "--short", "HEAD"]).unwrap();
        assert_eq!(commit(root), clean);

        fs::write(root.join("untracked"), "").unwrap();
        assert_eq!(commit(root), clean);

        fs::write(root.join("tracked"), "two").unwrap();
        assert_eq!(commit(root), format!("{clean}-dirty"));
    }

    #[test]
    fn the_target_comes_from_the_cargo_arguments() {
        let msvc = "x86_64-pc-windows-msvc";
        assert_eq!(target(&args(&["--release"])), None);
        assert_eq!(target(&args(&["--release", "--target", msvc])), Some(msvc));
        assert_eq!(
            target(&args(&["--target=x86_64-unknown-linux-gnu"])),
            Some("x86_64-unknown-linux-gnu")
        );
        assert!(is_msvc(target(&args(&["--target", msvc]))));
        assert!(!is_msvc(target(&args(&[]))));
    }
}
