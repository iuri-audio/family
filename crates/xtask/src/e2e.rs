//! `cargo xtask e2e`: runs the end-to-end checks, Reaper scripts that drive the plugin just built
//! in Reaper (see `docs/driving-reaper.md`), and fails unless every check passes.
//!
//! The checks are family's own, in `docs/reaper/checks/`, which every plugin runs, and the
//! plugin's, any `*.lua` in its `tests/reaper/`. Each runs in a new folder under the run's
//! folder, with a scratch copy of the developer's Reaper configuration whose plugin paths name
//! only that folder's copy of the bundles, so the check loads the build under test and nothing
//! the developer has installed is touched.
//!
//! A check ends with a verdict from `prelude.lua`: PASS, FAIL (the plugin misbehaved), ERROR
//! (the check couldn't run) or SKIP (it doesn't apply to the plugin). A run that leaves no verdict,
//! such as one the launcher timed out, is broken, like an ERROR: neither says anything about the
//! plugin.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result, bail};

use crate::Plugin;

/// The files from `docs/reaper/` a run needs, written into the run's folder: the launchers, the
/// prelude and family's checks.
const FILES: [(&str, &str); 8] = [
    (
        "prelude.lua",
        include_str!("../../../docs/reaper/prelude.lua"),
    ),
    (
        "run-wsl.sh",
        include_str!("../../../docs/reaper/run-wsl.sh"),
    ),
    (
        "run-wsl.ps1",
        include_str!("../../../docs/reaper/run-wsl.ps1"),
    ),
    (
        "run-flatpak.sh",
        include_str!("../../../docs/reaper/run-flatpak.sh"),
    ),
    (
        "run-linux.sh",
        include_str!("../../../docs/reaper/run-linux.sh"),
    ),
    (
        "checks/load.lua",
        include_str!("../../../docs/reaper/checks/load.lua"),
    ),
    (
        "checks/automation.lua",
        include_str!("../../../docs/reaper/checks/automation.lua"),
    ),
    (
        "checks/wheel.lua",
        include_str!("../../../docs/reaper/checks/wheel.lua"),
    ),
];

/// The cargo feature, which each plugin forwards to `family-ui`, that compiles in the test hooks
/// some checks need. Only the e2e build turns it on.
pub const HOOKS_FEATURE: &str = "e2e-hooks";

/// Where under `target/` the e2e build goes, so its bundles, which have the hooks, never sit
/// where a release build's do.
pub const TARGET_SUBDIR: &str = "e2e";

/// How a check's leading comments ask for environment variables for the plugin, such as a hook's:
/// `-- e2e-env: KEY=VALUE`.
const ENV_PREFIX: &str = "-- e2e-env: ";

/// How a check's leading comments say it can only run on a plugin with an editor.
const NEEDS_EDITOR: &str = "-- e2e-needs: editor";

/// Where a plugin keeps its own checks, under its workspace root.
const PLUGIN_CHECKS: &str = "tests/reaper";

/// How long a check may take before its launcher stops Reaper, unless `RUN_TIMEOUT` says.
const DEFAULT_TIMEOUT_S: &str = "90";

/// The Reaper a run drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Host {
    /// Reaper on Windows, from WSL.
    Wsl,
    /// Flathub's Reaper on Linux.
    Flatpak,
    /// Reaper installed natively on Linux.
    Linux,
}

impl Host {
    /// The host for a build with `flatpak` and the target triple `target`, or an error saying
    /// which builds the checks can run.
    pub fn for_build(flatpak: bool, target: Option<&str>) -> Result<Self> {
        let wsl = std::env::var_os("WSL_DISTRO_NAME").is_some();
        Self::for_machine(flatpak, target, wsl, cfg!(target_os = "linux"))
    }

    /// [`Self::for_build`] on a machine that is or isn't `wsl`, and does or doesn't run `linux`.
    fn for_machine(flatpak: bool, target: Option<&str>, wsl: bool, linux: bool) -> Result<Self> {
        if flatpak {
            return Ok(Self::Flatpak);
        }
        if crate::is_msvc(target) && wsl {
            return Ok(Self::Wsl);
        }
        // A native build: no target, or a Linux one. Not from WSL, whose Reaper is Windows'.
        if linux && !wsl && target.is_none_or(|target| target.contains("-linux-")) {
            return Ok(Self::Linux);
        }
        bail!(
            "e2e drives Reaper on Windows from WSL (--target x86_64-pc-windows-msvc), Flathub's \
             Reaper (--flatpak) or a Reaper installed natively on Linux (a native build)"
        )
    }

    /// The launcher's file name, among `FILES`.
    const fn launcher(self) -> &'static str {
        match self {
            Self::Wsl => "run-wsl.sh",
            Self::Flatpak => "run-flatpak.sh",
            Self::Linux => "run-linux.sh",
        }
    }

    /// The developer's Reaper configuration folder, which the run copies from.
    fn config_dir(self) -> Result<PathBuf> {
        match self {
            Self::Wsl => Ok(std::env::var_os("REAPER_CONFIG").map_or_else(
                || windows_home().join("AppData/Roaming/REAPER"),
                PathBuf::from,
            )),
            Self::Flatpak => Ok(home()?.join(".var/app/fm.reaper.Reaper/config/REAPER")),
            Self::Linux => match std::env::var_os("REAPER_CONFIG") {
                Some(config) => Ok(PathBuf::from(config)),
                None => Ok(home()?.join(".config/REAPER")),
            },
        }
    }

    /// The configuration file's name in that folder.
    const fn ini(self) -> &'static str {
        match self {
            Self::Wsl => "REAPER.ini",
            Self::Flatpak | Self::Linux => "reaper.ini",
        }
    }

    /// Reaper's plugin scan caches, emptied so a cached path can't select another build.
    const fn caches(self) -> [&'static str; 2] {
        match self {
            Self::Wsl => ["reaper-vstplugins64.ini", "reaper-clap-win64.ini"],
            Self::Flatpak | Self::Linux => {
                ["reaper-vstplugins64.ini", "reaper-clap-linux-x86_64.ini"]
            }
        }
    }

    /// The folder runs go under. Windows Reaper can't reliably write through the WSL share, so
    /// a WSL run goes on the Windows drive.
    fn runs_dir(self, plugin: &Plugin<'_>) -> PathBuf {
        let folder = format!("{}-e2e", plugin.name.to_lowercase());
        match self {
            Self::Wsl => windows_home().join("AppData/Local/Temp").join(folder),
            Self::Flatpak | Self::Linux => plugin.workspace_root.join("target").join(folder),
        }
    }

    /// `path` as this host's Reaper names it.
    fn host_path(self, path: &Path) -> Result<String> {
        match self {
            Self::Wsl => {
                let output = Command::new("wslpath")
                    .arg("-w")
                    .arg(path)
                    .output()
                    .context("could not run wslpath")?;
                anyhow::ensure!(
                    output.status.success(),
                    "wslpath failed on {}",
                    path.display()
                );
                Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
            }
            Self::Flatpak | Self::Linux => Ok(path.to_string_lossy().into_owned()),
        }
    }
}

/// The Windows user's home folder from WSL, as `run-wsl.sh` finds it.
fn windows_home() -> PathBuf {
    let user = std::env::var("USER").unwrap_or_default();
    Path::new("/mnt/c/Users").join(user)
}

fn home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is not set")
}

/// How a check ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The plugin did what the check expects.
    Pass,
    /// The plugin did something else.
    Fail(String),
    /// The check couldn't run, or left no verdict, so it says nothing about the plugin.
    Broken(String),
    /// The check doesn't apply to the plugin, such as one needing an editor on a plugin without.
    Skipped(String),
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pass => write!(f, "PASS"),
            Self::Fail(reason) => write!(f, "FAIL    {reason}"),
            Self::Broken(reason) => write!(f, "BROKEN  {reason}"),
            Self::Skipped(reason) => write!(f, "SKIP    {reason}"),
        }
    }
}

/// The outcome from a check's `verdict.txt`, `verdict`, or, if it wrote none, from the
/// launcher's exit `status`.
fn outcome(verdict: Option<&str>, status: Option<i32>) -> Outcome {
    if let Some(verdict) = verdict {
        let mut lines = verdict.lines();
        let word = lines.next().unwrap_or_default().trim();
        let reason = lines.next().unwrap_or_default().trim().to_owned();
        return match word {
            "PASS" => Outcome::Pass,
            "FAIL" => Outcome::Fail(reason),
            "ERROR" => Outcome::Broken(reason),
            "SKIP" => Outcome::Skipped(reason),
            _ => Outcome::Broken(format!("unknown verdict {word:?}")),
        };
    }
    Outcome::Broken(match status {
        Some(124) => "Reaper didn't quit in time: the check hung or a dialog is open".to_owned(),
        Some(status) => format!("no verdict; the launcher exited with {status}"),
        None => "no verdict; the launcher was stopped".to_owned(),
    })
}

/// A check: its name, the script, written into the run's folder, the environment its header
/// asks for, and whether its header says it needs an editor.
struct Check {
    name: String,
    script: PathBuf,
    env: Vec<String>,
    needs_editor: bool,
}

impl Check {
    /// Whether a plugin with or without an `editor` skips this check.
    const fn skipped_for(&self, editor: bool) -> bool {
        self.needs_editor && !editor
    }
}

/// The leading comment lines of a check's script.
fn header(script: &str) -> impl Iterator<Item = &str> {
    script.lines().take_while(|line| line.starts_with("--"))
}

/// The `KEY=VALUE` pairs a script's leading comments ask for.
fn env_of(script: &str) -> Vec<String> {
    header(script)
        .filter_map(|line| line.strip_prefix(ENV_PREFIX))
        .map(|pair| pair.trim().to_owned())
        .collect()
}

/// Whether a script's leading comments say it needs an editor.
fn needs_editor(script: &str) -> bool {
    header(script).any(|line| line.trim_end() == NEEDS_EDITOR)
}

/// Runs every check against the bundles in `bundled` in `host`'s Reaper, printing each outcome.
/// Fails unless they all pass, but for the checks that need an editor, which a plugin without one
/// skips.
pub fn run(plugin: &Plugin<'_>, bundled: &Path, host: Host) -> Result<()> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("the clock is before 1970")?
        .as_secs();
    // The run's folder name is in every path the bundles are loaded from, so a check can tell
    // the build under test from an installed one.
    let tag = format!("e2e-{stamp}");
    let run_dir = host.runs_dir(plugin).join(&tag);
    let reaper_dir = run_dir.join("reaper");
    for (path, text) in FILES {
        write(&reaper_dir.join(path), text.as_bytes())?;
    }
    let checks = checks(plugin, &reaper_dir)?;
    let config = host.config_dir()?.join(host.ini());
    let ini = fs::read(&config)
        .with_context(|| format!("can't read Reaper's configuration {}", config.display()))?;

    print(&format!(
        "running {} checks in {}",
        checks.len(),
        run_dir.display()
    ));
    let mut outcomes = Vec::with_capacity(checks.len());
    for check in &checks {
        if check.skipped_for(plugin.editor) {
            let outcome = Outcome::Skipped("needs an editor".to_owned());
            print(&format!("{:<12} {outcome}", check.name));
            outcomes.push(outcome);
            continue;
        }
        let out = run_dir.join(&check.name);
        let plugins = out.join("plugins");
        let mut copies = Vec::new();
        for bundle in [
            format!("{}.vst3", plugin.name),
            format!("{}.clap", plugin.name),
        ] {
            crate::install::copy_plan(&bundled.join(&bundle), &plugins.join(&bundle), &mut copies)
                .with_context(|| format!("can't read the bundle {bundle}"))?;
        }
        for (source, target) in copies {
            write(&target, &fs::read(&source)?)?;
        }
        let res = out.join("res");
        write(
            &res.join(host.ini()),
            &with_plugin_paths(&ini, &host.host_path(&plugins)?, host),
        )?;
        for cache in host.caches() {
            write(&res.join(cache), b"")?;
        }

        let status = Command::new("bash")
            .arg(reaper_dir.join(host.launcher()))
            .arg(&out)
            .arg(&check.script)
            .arg(format!("PLUGIN_NAME={}", plugin.name))
            .arg(format!("BUILD_TAG={tag}"))
            .arg(format!("PLUGIN_EDITOR={}", u8::from(plugin.editor)))
            .arg(match host {
                Host::Wsl => "E2E_HOST=wsl",
                Host::Flatpak => "E2E_HOST=flatpak",
                Host::Linux => "E2E_HOST=linux",
            })
            .args(&check.env)
            .env(
                "RUN_TIMEOUT",
                std::env::var("RUN_TIMEOUT").unwrap_or_else(|_| DEFAULT_TIMEOUT_S.to_owned()),
            )
            .status()
            .context("could not run bash")?;
        let verdict = fs::read_to_string(out.join("verdict.txt")).ok();
        let outcome = outcome(verdict.as_deref(), status.code());
        print(&format!("{:<12} {outcome}", check.name));
        outcomes.push(outcome);
    }

    print(&summary(&outcomes, &run_dir)?);
    Ok(())
}

/// The line that sums up a run's `outcomes`. Fails unless every check that ran passed.
fn summary(outcomes: &[Outcome], run_dir: &Path) -> Result<String> {
    let count = |wanted: fn(&Outcome) -> bool| outcomes.iter().filter(|&o| wanted(o)).count();
    let failed = count(|outcome| matches!(outcome, Outcome::Fail(_) | Outcome::Broken(_)));
    let skipped = count(|outcome| matches!(outcome, Outcome::Skipped(_)));
    let ran = outcomes.len() - skipped;
    if failed > 0 {
        bail!(
            "{failed} of {ran} checks didn't pass: see result.txt, nice.log and reaper.stdout in {}",
            run_dir.display()
        );
    }
    Ok(if skipped > 0 {
        format!("all {ran} checks passed; {skipped} skipped")
    } else {
        format!("all {ran} checks passed")
    })
}

/// Family's checks, then the plugin's, sorted by name. The plugin's are copied beside family's,
/// so a sandboxed Reaper can read them.
fn checks(plugin: &Plugin<'_>, reaper_dir: &Path) -> Result<Vec<Check>> {
    let mut checks: Vec<Check> = FILES
        .iter()
        .filter_map(|(path, text)| Some((path.strip_prefix("checks/")?, text)))
        .map(|(file, text)| Check {
            name: file.trim_end_matches(".lua").to_owned(),
            script: reaper_dir.join("checks").join(file),
            env: env_of(text),
            needs_editor: needs_editor(text),
        })
        .collect();
    let own = plugin.workspace_root.join(PLUGIN_CHECKS);
    let mut scripts: Vec<PathBuf> = match fs::read_dir(&own) {
        Ok(entries) => entries
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<_, _>>()?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error).context(format!("can't list {}", own.display())),
    };
    scripts.retain(|path| path.extension().is_some_and(|extension| extension == "lua"));
    scripts.sort();
    for source in scripts {
        let name = source
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        if checks.iter().any(|check| check.name == name) {
            bail!(
                "{} has the name of one of family's checks",
                source.display()
            );
        }
        let script = reaper_dir
            .join("plugin")
            .join(source.file_name().unwrap_or_default());
        let text = fs::read_to_string(&source)
            .with_context(|| format!("can't read {}", source.display()))?;
        write(&script, text.as_bytes())?;
        checks.push(Check {
            name,
            script,
            env: env_of(&text),
            needs_editor: needs_editor(&text),
        });
    }
    Ok(checks)
}

/// The configuration `ini` with every VST3 and CLAP plugin path set to `folder`, so Reaper scans
/// nothing else. Missing paths are added in the Reaper section, overriding the host's defaults.
fn with_plugin_paths(ini: &[u8], folder: &str, host: Host) -> Vec<u8> {
    let is_path = |line: &[u8]| {
        let key = line.split(|&byte| byte == b'=').next().unwrap_or_default();
        line.contains(&b'=') && (key.starts_with(b"vstpath") || key.starts_with(b"clap_path"))
    };
    let mut lines: Vec<Vec<u8>> = ini
        .split(|&byte| byte == b'\n')
        .map(|line| {
            if is_path(line) {
                let key = line.split(|&byte| byte == b'=').next().unwrap_or_default();
                [key, b"=", folder.as_bytes()].concat()
            } else {
                line.to_vec()
            }
        })
        .collect();
    let section = lines
        .iter()
        .position(|line| line.trim_ascii().eq_ignore_ascii_case(b"[reaper]"))
        .unwrap_or_else(|| {
            lines.push(b"[reaper]".to_vec());
            lines.len() - 1
        });
    let keys = match host {
        Host::Wsl => ["vstpath64", "clap_path_win64"],
        Host::Flatpak | Host::Linux => ["vstpath", "clap_path_linux-x86_64"],
    };
    for key in keys {
        let prefix = format!("{key}=");
        if !lines.iter().any(|line| line.starts_with(prefix.as_bytes())) {
            let at = section + 1;
            lines.insert(at, format!("{prefix}{folder}").into_bytes());
        }
    }
    if host != Host::Wsl {
        // Reaper 7.82 adds ~/.vst3 on its first VST3 scan unless this migration bit is set.
        // With that bit absent, setting the path alone is not enough. Keep every other flag
        // (837 is Reaper's default without the bit).
        let at = lines
            .iter()
            .position(|line| line.starts_with(b"vstfullstate="));
        let flags = at
            .and_then(|at| lines.get(at))
            .and_then(|line| std::str::from_utf8(line).ok())
            .and_then(|line| line.strip_prefix("vstfullstate="))
            .and_then(|value| value.trim().parse::<u32>().ok())
            .unwrap_or(837);
        let setting = format!("vstfullstate={}", flags | 8192).into_bytes();
        if let Some(line) = at.and_then(|at| lines.get_mut(at)) {
            *line = setting;
        } else {
            lines.insert(section + 1, setting);
        }
    }
    lines.join(&b'\n')
}

/// Writes `contents` to `path`, creating its folder.
fn write(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("can't create {}", parent.display()))?;
    }
    fs::write(path, contents).with_context(|| format!("can't write {}", path.display()))
}

#[expect(
    clippy::print_stdout,
    reason = "reporting the checks is what the command is for"
)]
fn print(line: &str) {
    println!("{line}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_verdict_gives_the_outcome() {
        assert_eq!(outcome(Some("PASS\nall good\n"), Some(0)), Outcome::Pass);
        assert_eq!(
            outcome(Some("FAIL\nWidth moved\n"), Some(0)),
            Outcome::Fail("Width moved".to_owned())
        );
        assert_eq!(
            outcome(Some("ERROR\nno audio device\n"), Some(0)),
            Outcome::Broken("no audio device".to_owned())
        );
        assert_eq!(
            outcome(Some("SKIP\nno parameter\n"), Some(0)),
            Outcome::Skipped("no parameter".to_owned())
        );
    }

    #[test]
    fn an_outcome_prints_its_word_and_reason() {
        assert!(Outcome::Pass.to_string().starts_with("PASS"));
        for (outcome, word) in [
            (Outcome::Fail("why".to_owned()), "FAIL"),
            (Outcome::Broken("why".to_owned()), "BROKEN"),
            (Outcome::Skipped("why".to_owned()), "SKIP"),
        ] {
            let printed = outcome.to_string();
            assert!(
                printed.starts_with(word) && printed.contains("why"),
                "{printed}"
            );
        }
    }

    #[test]
    fn a_run_passes_only_if_every_check_that_ran_passed() {
        let run = Path::new("/run");
        let skip = || Outcome::Skipped(String::new());

        assert_eq!(
            summary(&[Outcome::Pass, Outcome::Pass], run).unwrap(),
            "all 2 checks passed"
        );
        assert_eq!(
            summary(&[Outcome::Pass, Outcome::Pass, skip()], run).unwrap(),
            "all 2 checks passed; 1 skipped"
        );
        assert_eq!(
            summary(&[skip()], run).unwrap(),
            "all 0 checks passed; 1 skipped"
        );
        let error = summary(
            &[
                Outcome::Pass,
                Outcome::Fail(String::new()),
                Outcome::Broken(String::new()),
                skip(),
            ],
            run,
        )
        .unwrap_err();
        assert!(
            error.to_string().starts_with("2 of 3 checks didn't pass"),
            "{error}"
        );
        summary(&[Outcome::Broken(String::new())], run).unwrap_err();
    }

    #[test]
    fn only_a_plugin_without_an_editor_skips_a_check_that_needs_one() {
        let check = |needs_editor| Check {
            name: String::new(),
            script: PathBuf::new(),
            env: Vec::new(),
            needs_editor,
        };

        assert!(check(true).skipped_for(false));
        assert!(!check(true).skipped_for(true));
        assert!(!check(false).skipped_for(false));
        assert!(!check(false).skipped_for(true));
    }

    const fn plugin(workspace_root: &Path) -> Plugin<'_> {
        Plugin {
            package: "fake",
            name: "Fake",
            install_dir_variable: "FAKE_INSTALL_DIR",
            workspace_root,
            editor: true,
        }
    }

    fn check_names(checks: &[Check]) -> Vec<&str> {
        checks.iter().map(|check| check.name.as_str()).collect()
    }

    #[test]
    fn a_plugin_without_checks_of_its_own_runs_familys() {
        let root = tempfile::tempdir().unwrap();
        let reaper = root.path().join("reaper");

        let checks = checks(&plugin(root.path()), &reaper).unwrap();

        assert_eq!(check_names(&checks), ["load", "automation", "wheel"]);
        assert_eq!(checks[0].script, reaper.join("checks/load.lua"));
    }

    #[test]
    fn a_plugins_lua_scripts_are_copied_beside_familys_and_run_after_them() {
        let root = tempfile::tempdir().unwrap();
        let own = root.path().join(PLUGIN_CHECKS);
        fs::create_dir_all(&own).unwrap();
        fs::write(own.join("second.lua"), "-- e2e-env: A=1\n").unwrap();
        fs::write(own.join("first.lua"), format!("{NEEDS_EDITOR}\n")).unwrap();
        fs::write(own.join("notes.txt"), "not a check").unwrap();
        let reaper = root.path().join("reaper");

        let checks = checks(&plugin(root.path()), &reaper).unwrap();

        assert_eq!(
            check_names(&checks),
            ["load", "automation", "wheel", "first", "second"]
        );
        assert!(checks[3].needs_editor);
        assert_eq!(checks[4].env, ["A=1"]);
        assert_eq!(
            fs::read_to_string(reaper.join("plugin/second.lua")).unwrap(),
            "-- e2e-env: A=1\n"
        );
    }

    #[test]
    fn a_plugins_check_cant_take_the_name_of_one_of_familys() {
        let root = tempfile::tempdir().unwrap();
        let own = root.path().join(PLUGIN_CHECKS);
        fs::create_dir_all(&own).unwrap();
        fs::write(own.join("load.lua"), "").unwrap();

        let error = checks(&plugin(root.path()), &root.path().join("reaper"))
            .err()
            .expect("the clash is refused");

        assert!(error.to_string().contains("family's checks"), "{error}");
    }

    #[test]
    fn a_plugins_checks_folder_that_cant_be_listed_is_an_error_not_an_empty_folder() {
        let root = tempfile::tempdir().unwrap();
        let own = root.path().join(PLUGIN_CHECKS);
        fs::create_dir_all(own.parent().unwrap()).unwrap();
        fs::write(&own, "a file, not a folder").unwrap();

        let error = checks(&plugin(root.path()), &root.path().join("reaper"))
            .err()
            .expect("the folder can't be listed");

        assert!(error.to_string().contains("can't list"), "{error}");
    }

    #[test]
    fn every_hosts_launcher_is_among_the_files_a_run_writes() {
        for host in [Host::Wsl, Host::Flatpak, Host::Linux] {
            assert!(FILES.iter().any(|(path, _)| *path == host.launcher()));
        }
    }

    #[test]
    fn runs_go_under_the_workspace_or_on_the_windows_drive() {
        let root = Path::new("/plugin");
        let plugin = plugin(root);

        assert_eq!(
            Host::Linux.runs_dir(&plugin),
            Path::new("/plugin/target/fake-e2e")
        );
        let wsl = Host::Wsl.runs_dir(&plugin);
        assert!(wsl.starts_with("/mnt/c/Users"), "{}", wsl.display());
        assert!(
            wsl.ends_with("AppData/Local/Temp/fake-e2e"),
            "{}",
            wsl.display()
        );
    }

    #[test]
    fn a_native_reaper_names_a_path_as_it_is() {
        assert_eq!(Host::Linux.host_path(Path::new("/a/b")).unwrap(), "/a/b");
    }

    #[test]
    fn no_verdict_is_broken_whatever_the_launcher_says() {
        assert!(matches!(outcome(None, Some(0)), Outcome::Broken(_)));
        assert!(
            matches!(outcome(None, Some(124)), Outcome::Broken(reason) if reason.contains("in time"))
        );
        assert!(matches!(outcome(None, None), Outcome::Broken(_)));
    }

    #[test]
    fn a_header_asks_for_environment_variables() {
        let script = "-- e2e-env: FAMILY_E2E_WHEEL=8\n-- e2e-env: OTHER=a b\n--\n-- about\n\
                      local x = 1\n-- e2e-env: LATE=1\n";
        assert_eq!(env_of(script), ["FAMILY_E2E_WHEEL=8", "OTHER=a b"]);
        assert_eq!(env_of("local x = 1\n"), Vec::<String>::new());
    }

    #[test]
    fn the_wheel_check_asks_for_its_hook() {
        let (_, wheel) = FILES
            .iter()
            .find(|(path, _)| *path == "checks/wheel.lua")
            .unwrap();
        assert_eq!(env_of(wheel), ["FAMILY_E2E_WHEEL=8"]);
    }

    #[test]
    fn a_header_says_whether_a_check_needs_an_editor() {
        assert!(needs_editor(
            "-- e2e-env: A=1\n-- e2e-needs: editor\nlocal x = 1\n"
        ));
        assert!(!needs_editor(
            "-- about the editor\nlocal x = 1\n-- e2e-needs: editor\n"
        ));
    }

    #[test]
    fn only_the_wheel_check_needs_an_editor() {
        let needing: Vec<&str> = FILES
            .iter()
            .filter(|(path, text)| path.starts_with("checks/") && needs_editor(text))
            .map(|(path, _)| *path)
            .collect();
        assert_eq!(needing, ["checks/wheel.lua"]);
    }

    #[test]
    fn a_native_linux_build_runs_in_the_native_reaper() {
        for target in [None, Some("x86_64-unknown-linux-gnu")] {
            let host = Host::for_machine(false, target, false, true).unwrap();
            assert_eq!(host, Host::Linux);
        }
    }

    #[test]
    fn flatpak_and_wsl_builds_keep_their_hosts() {
        let msvc = Some("x86_64-pc-windows-msvc");

        assert_eq!(
            Host::for_machine(true, None, false, true).unwrap(),
            Host::Flatpak
        );
        assert_eq!(
            Host::for_machine(false, msvc, true, true).unwrap(),
            Host::Wsl
        );
    }

    #[test]
    fn builds_with_no_reaper_to_run_them_are_refused() {
        let msvc = Some("x86_64-pc-windows-msvc");

        // A native build from WSL, whose Reaper is Windows'; a Windows build off WSL; any build
        // off Linux without the Flatpak.
        Host::for_machine(false, None, true, true).unwrap_err();
        Host::for_machine(false, msvc, false, true).unwrap_err();
        Host::for_machine(false, None, false, false).unwrap_err();
    }

    #[test]
    fn a_native_linux_config_gets_the_vst3_migration_bit_too() {
        let patched = with_plugin_paths(b"[REAPER]\nvstfullstate=837\n", "/p", Host::Linux);

        let text = String::from_utf8(patched).unwrap();
        assert!(text.contains("vstfullstate=9029"), "{text}");
        assert!(text.contains("vstpath=/p"), "{text}");
        assert!(text.contains("clap_path_linux-x86_64=/p"), "{text}");
    }

    #[test]
    fn every_plugin_path_names_only_the_folder() {
        let ini = b"[REAPER]\nvstpath64=C:\\VST3;D:\\More\nclap_path_win64=%CLAP_PATH%\nvstpath_x=1\nother=vstpath64\n";
        let patched = with_plugin_paths(ini, "C:\\run\\plugins", Host::Wsl);
        assert_eq!(
            String::from_utf8(patched).unwrap(),
            "[REAPER]\nvstpath64=C:\\run\\plugins\nclap_path_win64=C:\\run\\plugins\n\
             vstpath_x=C:\\run\\plugins\nother=vstpath64\n"
        );
    }

    #[test]
    fn missing_windows_paths_are_added_to_the_reaper_section() {
        let patched = with_plugin_paths(b"[REAPER]\nother=1\n", "C:\\p", Host::Wsl);
        assert_eq!(
            String::from_utf8(patched).unwrap(),
            "[REAPER]\nclap_path_win64=C:\\p\nvstpath64=C:\\p\nother=1\n"
        );
    }

    #[test]
    fn missing_linux_paths_override_defaults_in_the_reaper_section() {
        for section in ["[reaper]", "[REAPER]"] {
            let ini = format!("[.swell]\nscale=1\n{section}\nother=1\n[Recent]\n");
            let patched = with_plugin_paths(ini.as_bytes(), "/run/plugins", Host::Flatpak);
            assert_eq!(
                String::from_utf8(patched).unwrap(),
                format!(
                    "[.swell]\nscale=1\n{section}\nvstfullstate=9029\nclap_path_linux-x86_64=/run/plugins\nvstpath=/run/plugins\nother=1\n[Recent]\n"
                )
            );
        }
    }

    #[test]
    fn a_missing_reaper_section_is_created() {
        let patched = with_plugin_paths(b"[Recent]\nother=1\n", "/p", Host::Flatpak);
        assert_eq!(
            String::from_utf8(patched).unwrap(),
            "[Recent]\nother=1\n\n[reaper]\nvstfullstate=9029\nclap_path_linux-x86_64=/p\nvstpath=/p"
        );
    }

    #[test]
    fn linux_vst3_migration_preserves_the_other_flags() {
        for flags in [0, 837, 9029, 65535] {
            let ini = format!("[reaper]\nvstfullstate={flags}\n");
            let patched = with_plugin_paths(ini.as_bytes(), "/p", Host::Flatpak);
            let patched = String::from_utf8(patched).unwrap();
            assert!(patched.contains(&format!("vstfullstate={}\n", flags | 8192)));
            assert_eq!(patched.matches("vstfullstate=").count(), 1);
        }
    }

    #[test]
    fn non_utf8_settings_survive() {
        let ini = b"[REAPER]\nname=caf\xe9\nvstpath64=x\n";
        let patched = with_plugin_paths(ini, "y", Host::Wsl);
        assert_eq!(
            patched,
            b"[REAPER]\nclap_path_win64=y\nname=caf\xe9\nvstpath64=y\n".to_vec()
        );
    }
}
