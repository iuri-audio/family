# Driving Reaper

When a bug only shows in a host (the editor, automation, transport, latency, state saved with a project), reproduce it yourself in Reaper with a ReaScript, instead of asking the developer to click through it. A script runs the same steps every time, as many times as it takes, under load if timing matters, and leaves its results in files you read.

## Steps

1. **Install the build under test** with Reaper closed, as the plugin's README says under *Installing* for your platform. The plugin folders are shared with the developer's own Reaper, so when you've finished, reinstall the build they expect.
2. **Write a script that turns the bug into a number.** It sets up what the bug needs through the [ReaScript API](https://www.reaper.fm/sdk/reascript/reascripthelp.html) (tracks, FX, playback, automation, project changes), repeats the suspect step, and writes what it observed to `$OUT`. Start from [`reaper/editor-cycles.lua`](reaper/editor-cycles.lua), which opens and closes an editor N times, or [`reaper/touch.lua`](reaper/touch.lua), which checks that gestures in touch mode let go. The script is done when a failing run and a passing run produce different output, such as "drew 26 of 30".
3. **Run it** with your platform's launcher (below): `<launcher> <out-dir> <script.lua> KEY=VALUE...`. Each `KEY=VALUE` reaches the script as an environment variable, beside `OUT`, `NICE_LOG` and `PRELUDE`. Give each run a new `<out-dir>`: the launcher refuses one that holds an earlier run's results.
4. **Read the results**: the script's own files in `<out-dir>`, the plugin's log in `<out-dir>/nice.log` (family-ui logs each editor opening and how it settled), and Reaper's output in `<out-dir>/reaper.stdout`.

## Launchers

A launcher runs Reaper on a scratch copy of the developer's configuration (settings, licence and plugin scan), so a run never changes theirs; passes the environment to the script; refuses to start while the developer's Reaper is open; and exits 124 when Reaper hasn't quit within `RUN_TIMEOUT` seconds (default 120).

- **Linux, Reaper from Flathub**: [`reaper/run-flatpak.sh`](reaper/run-flatpak.sh).
- **WSL, Reaper on Windows**: `bash reaper/run-wsl.sh <out-dir> <script.lua> KEY=VALUE...`. Use an output folder on a Windows drive (`/mnt/c/...`): Reaper cannot reliably write a configuration through the WSL network share. `REAPER_EXE` and `REAPER_CONFIG` override the default installation and configuration paths; `REAPER_RESOURCE_DIR` can reuse a scratch resource folder between runs to avoid repeating Reaper's first-run resource setup. Never point it at the developer's real resource folder. The launcher disables a copied default-project template so a missing template cannot block the script. Its PowerShell helper stops only the process it launched on timeout.

## The script's contract

- **Load [`reaper/prelude.lua`](reaper/prelude.lua)** with `local drive = dofile(os.getenv("PRELUDE"))` and use what it gives: `drive.start(fn)` waits until Reaper can play (playback started in its first seconds stops at once at 0 s); `drive.say(...)` writes a line to `$OUT/result.txt`; `drive.quit()` quits with nothing to save, since quitting a changed project opens a save dialog that someone has to click.
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

Each run opens Reaper's windows on the developer's screen and takes focus for its length. Keep runs short, and say before a long one.
