# Driving Reaper

When a bug only shows in a host (the editor, automation, transport, latency, state saved with a project), reproduce it yourself in Reaper with a ReaScript, instead of asking the developer to click through it. A script runs the same steps every time, as many times as it takes, under load if timing matters, and leaves its results in files you read.

## Steps

1. **Install the build under test** with Reaper closed, as the plugin's README says under *Installing* for your platform. The plugin folders are shared with the developer's own Reaper, so when you've finished, reinstall the build they expect.
2. **Write a script that turns the bug into a number.** It sets up what the bug needs through the [ReaScript API](https://www.reaper.fm/sdk/reascript/reascripthelp.html) (tracks, FX, playback, automation, project changes), repeats the suspect step, and writes what it observed to `$OUT`. Start from [`reaper/editor-cycles.lua`](reaper/editor-cycles.lua) for repeated openings, [`reaper/touch.lua`](reaper/touch.lua) for gestures in touch mode, or the [saved-project/new-instance check](#saved-projects-and-new-instances-windows) for cropping and scale bugs. The script is done when a failing run and a passing run produce different output, such as "drew 26 of 30".
3. **Run it** with your platform's launcher (below): `<launcher> <out-dir> <script.lua> KEY=VALUE...`. Each `KEY=VALUE` reaches the script as an environment variable, beside `OUT`, `NICE_LOG` and `PRELUDE`. Give each run a new `<out-dir>`: the launcher refuses one that holds an earlier run's results.
4. **Read the results**: the script's own files in `<out-dir>`, the plugin's log in `<out-dir>/nice.log` (family-ui logs each editor opening and how it settled), and Reaper's output in `<out-dir>/reaper.stdout`.

## The end-to-end checks

`cargo xtask e2e` (from a plugin's repo, with the same arguments as `bundle`) builds the plugin and runs a fixed set of checks in Reaper against that build, then exits non-zero unless every one passes. Run it before bumping a plugin's family rev and before a release:

```sh
cargo xtask e2e --release --target x86_64-pc-windows-msvc   # WSL: Windows build, Windows Reaper
cargo xtask e2e --release --flatpak                         # Linux: Flatpak build, Flathub's Reaper
```

The two test different builds in different hosts (window code, display scaling, audio), so a pass on one says nothing about the other.

**The test hooks.** The build is the release profile with the test hooks compiled in: family-ui's `e2e-hooks` feature, which each plugin forwards as its own `e2e-hooks` feature (`e2e-hooks = ["family-ui/e2e-hooks"]` in its plugin crate). It goes to `target/e2e/`, never `target/bundled/`, so it can't be installed or shipped by mistake. A hook does nothing unless its environment variable, named `FAMILY_E2E_*`, is set when Reaper starts. A check asks for one in its leading comments with `-- e2e-env: KEY=VALUE`; that's also why each check runs in a Reaper of its own. Each plugin's CI builds its release library and fails if `FAMILY_E2E_` appears in it.

- **Family's checks**, in [`reaper/checks/`](reaper/checks/), run for every plugin: `load` (each format loads from the build under test, its editor opens, a parameter the host sets stays set, the editor reopens) `automation` (with the editor open, a parameter follows a ramp of automation through playback) and `wheel` (through the wheel hook, in touch mode: a burst of notches is recorded whole, and the parameter returns to the automation after the burst and when the editor closes mid-burst; it fails on the build before the fix for family issue 2). They exercise `$PARAM` if set, else the first continuous parameter, which for `wheel` must have a dial.
- **A plugin's checks** are the `*.lua` files in its `tests/reaper/`.

Each check runs in a new folder under the run's folder (printed at the start; on WSL under `%LOCALAPPDATA%\Temp`), with a copy of the bundles and a scratch configuration whose plugin paths name only that copy, so it never loads or changes the installed build. A check ends with `drive.pass`, `drive.fail` (the plugin misbehaved) or `drive.error` (the check couldn't run). A check that leaves no verdict, such as one that hangs until `RUN_TIMEOUT` (default 90 s here), is reported BROKEN: like an ERROR, it says nothing about the plugin, so fix the check. On Linux the plugin-path settings' names are unverified: `load` fails if a VST3 loads from elsewhere, but can't tell which CLAP loaded.

**Adding a check.** When a script reproduces a host bug with a clear verdict, and it fails on the build before the fix and passes after, it belongs in the suite: in family's `reaper/checks/` (and `FILES` in `crates/xtask/src/e2e.rs`) if it takes the plugin as a parameter, else in the plugin's `tests/reaper/`. When a check needs input a script can't give, add a hook in `crates/ui/src/e2e.rs`, behind the feature and a `FAMILY_E2E_*` variable, leaving the code path unchanged while the variable is unset. A check needing a person's eyes or hands (like [`touch.lua`](reaper/touch.lua)) stays a separate script.

## Launchers

A launcher runs Reaper on a scratch copy of the developer's configuration (settings, licence and plugin scan), so a run never changes theirs; passes the environment to the script; refuses to start while the developer's Reaper is open; and exits 124 when Reaper hasn't quit within `RUN_TIMEOUT` seconds (default 120).

- **Linux, Reaper from Flathub**: [`reaper/run-flatpak.sh`](reaper/run-flatpak.sh).
- **WSL, Reaper on Windows**: `bash reaper/run-wsl.sh <out-dir> <script.lua> KEY=VALUE...`. Use an output folder on a Windows drive (`/mnt/c/...`): Reaper cannot reliably write a configuration through the WSL network share. `REAPER_EXE` and `REAPER_CONFIG` override the default installation and configuration paths; `REAPER_RESOURCE_DIR` can reuse a scratch resource folder between runs to avoid repeating Reaper's first-run resource setup. Never point it at the developer's real resource folder. The launcher disables a copied default-project template so a missing template cannot block the script. Its PowerShell helper stops only the process it launched on timeout.

## Saved projects and new instances (Windows)

Reopening one instance can pass while a newly added instance opens cropped. [`reaper/new-instance.lua`](reaper/new-instance.lua) saves one instance, closes and reloads its project, opens that saved instance, then adds three new instances and opens each twice. It plays no audio. [`reaper/run-wsl-new-instance.sh`](reaper/run-wsl-new-instance.sh) runs it alongside the Windows checker, waiting for a physical host/canvas measurement at each step before closing that window.

```sh
bash docs/reaper/run-wsl-new-instance.sh /mnt/c/path/to/new-run 'VST3:YourPlugin' 1080 630 144
# Repeat in another new folder with 'CLAP:YourPlugin'.
```

The dimensions are physical pixels; the optional final argument asserts DPI (144 means 150%). Derive dimensions from the plugin's design size × user scale × display scale. `NEW_INSTANCES` changes the number of new instances; `EDITOR_TITLE` overrides the inferred `*YourPlugin*` window-title pattern. The existing launcher's configuration and timeout overrides also apply. The output directory must be new.

Each run has `2 + 2 × NEW_INSTANCES` measured phases: the seed instance, the reloaded instance, and each new instance's first opening and reopening. `windows.json` retains the actual host and baseview client sizes per phase; `measurement.stdout` reports each result. Exit 0 means every phase passed, 2 means a measured count/size/DPI mismatch, and other nonzero statuses indicate a scenario or launcher failure. Read `result.txt` and `nice.log` too: physical size alone cannot prove the canvas zoom is right.

To test without replacing installed plugins, copy the bundles to a temporary Windows folder. In a scratch resource directory, copy `REAPER.ini`, set `vstpath64` and `clap_path_win64` to that plugin folder, and create empty `reaper-vstplugins64.ini` and `reaper-clap-win64.ini` files so cached paths cannot select another build. Point `REAPER_RESOURCE_DIR` there. The launcher copies the licence if needed and disables the copied default-project template. `result.txt` lists matching installed plugin identifiers; verify the VST3 path names the intended build.

This scenario reproduced [Doubler #41](https://github.com/iuri-audio/doubler/issues/41#issuecomment-6245439487): the old build gave each new instance a 720×420 host around a 1080×630 child, while reopening fixed it. The corrected build passed every phase in VST3 and CLAP.

## The script's contract

- **Load [`reaper/prelude.lua`](reaper/prelude.lua)** with `local drive = dofile(os.getenv("PRELUDE"))` and use what it gives: `drive.start(fn)` waits until Reaper can play (playback started in its first seconds stops at once at 0 s); `drive.say(...)` writes a line to `$OUT/result.txt`; `drive.quit()` quits with nothing to save, since quitting a changed project opens a save dialog that someone has to click; `drive.pass`, `drive.fail` and `drive.error` give a check its verdict and quit; `drive.after(seconds, fn)` waits without blocking; `drive.defer(fn)` is `reaper.defer` that turns a Lua error into an ERROR verdict instead of a dialog that holds the run.
- **Keep files under `$OUT` or the script's folder.** The Flatpak Reaper runs sandboxed, and its launcher only shares those two (and the home folder).
- **Time with `reaper.defer`,** never by blocking. Reaper only draws and handles windows between deferred calls.
- **Check that a device change reached Reaper** with `reaper.GetAudioDeviceInfo("SRATE")` and `("BSIZE")`. On Linux, PipeWire's `pw-metadata` settings don't reach it (its JACK client keeps the block size it asked for, and the Deck allows only 48 kHz), so changing the sample rate or block size takes the developer in Reaper's Preferences.

## Checks that need the developer

Where only a person can act (mouse gestures on a plugin's editor) or see (a lamp, a glitch), the script does everything else and prompts them with `reaper.MB` at the moment to act or watch, then reads the result itself; `reaper.MB(question, title, 4)` asks Yes/No. Before the run, explain in plain words what the check is about (what touch automation is, what "latched" means), since the developer may not know the host's terms. In the run, nothing plays or moves before the first prompt, and each prompt says what to do and what to watch for, then starts on OK.

## Runs worth trusting

- **Measure the host as well as the canvas.** A correct zoom log does not prove the host's window is large enough. On Windows, run `reaper/check-editor-windows.ps1` alongside the editor cycles, passing `-Title '*<plugin name>*' -Width <physical width> -Height <physical height> -Count <cycles> -Output <json path>`. It retains each window's final client size and checks both Reaper's parent and the baseview child. `-Seconds` must cover startup and every cycle. It sets a per-monitor DPI context so the measurements are physical pixels, not DPI-virtualised coordinates. The plugin's zoom log is still needed to catch a correctly sized window with an enlarged canvas inside it.
- **Load** brings timing bugs out. While the script runs, keep every core busy, e.g. `for i in $(seq $(nproc)); do timeout 90 sh -c 'while :; do :; done' & done` (in WSL too: its processes share the machine's cores).
- **Count before calling a fix good.** At a failure rate of p, about 3/p clean runs in a row give 95 % confidence: about 35 at 1 in 12.
- **Check that the run exercised the fix**, from the logs, not only that it passed.
- **Compare** with the build before the fix, run the same way: if it doesn't fail, the script doesn't reproduce the bug.
- **Account for native scaling when reading zoom logs.** In the #41 check, VST3 retained zoom 1.5, while CLAP changed from 1.5 to 1 on reopening as native display scaling took over; both correctly measured 1080×630 at 144 DPI. A zoom value alone is not a cross-format acceptance check.
- **Keep the tested binaries.** Copy each build's bundles before building its comparison, and record its commit, pinned family revision and hashes. Windows relinking can change binary hashes at the same source revision; restore the saved bundles when exact tested bytes matter.

Each run opens Reaper's windows on the developer's screen and takes focus for its length. Keep runs short, and say before a long one.
