# Driving Reaper

When a bug only shows in a host (the editor, automation, transport, latency, state saved with a project), reproduce it yourself in Reaper with a ReaScript, instead of asking the developer to click through it. A script runs the same steps every time, as many times as it takes, under load if timing matters, and leaves its results in files you read.

## Steps

1. **Install the build under test** with Reaper closed, as the plugin's README says under *Installing* for your platform. The plugin folders are shared with the developer's own Reaper, so when you've finished, reinstall the build they expect.
2. **Write a script that turns the bug into a number.** It sets up what the bug needs through the [ReaScript API](https://www.reaper.fm/sdk/reascript/reascripthelp.html) (tracks, FX, playback, automation, project changes), repeats the suspect step, and writes what it observed to `$OUT`. Start from [`reaper/editor-cycles.lua`](reaper/editor-cycles.lua), which opens and closes an editor N times. The script is done when a failing run and a passing run produce different output, such as "drew 26 of 30".
3. **Run it** with your platform's launcher (below): `<launcher> <out-dir> <script.lua> KEY=VALUE...`. Each `KEY=VALUE` reaches the script as an environment variable, beside `OUT` and `NICE_LOG`.
4. **Read the results**: the script's own files in `<out-dir>`, the plugin's log in `<out-dir>/nice.log` (family-ui logs each editor opening and how it settled), and Reaper's output in `<out-dir>/reaper.stdout`.

## Launchers

A launcher runs Reaper on a scratch copy of the developer's configuration (settings, licence and plugin scan), so a run never changes theirs; passes the environment to the script; refuses to start while the developer's Reaper is open; and exits 124 when Reaper hasn't quit within `RUN_TIMEOUT` seconds (default 120).

- **Linux, Reaper from Flathub**: [`reaper/run-flatpak.sh`](reaper/run-flatpak.sh).
- **WSL, Reaper on Windows**: no launcher yet. Write `reaper/run-wsl.sh` to the same contract the first time you need it, check it against a real run, and commit it beside the Flatpak one. The pieces that differ: Reaper is `reaper.exe` under `/mnt/c/Program Files/REAPER (x64)/`; its configuration is in `%APPDATA%\REAPER` unless the install is portable; paths given to it go through `wslpath -w`; the environment reaches it through `WSLENV` (with `/p` on path variables); and `tasklist.exe` and `taskkill.exe` stand in for `pgrep` and stopping it after a timeout.

## The script's contract

- **Quit on its own, without a prompt.** End with the `quit` in `editor-cycles.lua`: it opens an empty project with `noprompt:` and then quits. Quitting a changed project otherwise opens a save dialog, and someone has to click it.
- **Keep files under `$OUT` or the script's folder.** The Flatpak Reaper runs sandboxed, and its launcher only shares those two (and the home folder).
- **Time with `reaper.defer`,** never by blocking. Reaper only draws and handles windows between deferred calls.

## Runs worth trusting

- **Load** brings timing bugs out. While the script runs, keep every core busy, e.g. `for i in $(seq $(nproc)); do timeout 90 sh -c 'while :; do :; done' & done` (in WSL too: its processes share the machine's cores).
- **Count before calling a fix good.** At a failure rate of p, about 3/p clean runs in a row give 95 % confidence: about 35 at 1 in 12.
- **Check that the run exercised the fix**, from the logs, not only that it passed.
- **Compare** with the build before the fix, run the same way: if it doesn't fail, the script doesn't reproduce the bug.

Each run opens Reaper's windows on the developer's screen and takes focus for its length. Keep runs short, and say before a long one.
