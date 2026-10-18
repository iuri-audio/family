# family

Code shared by a family of guitar plugins: Doubler, Quiet and Drop so far. The family has no name yet; this repo will be renamed `<name>-core` when it has one.

| Crate | What it holds |
|---|---|
| `family-dsp` | `no_std` DSP: the crossover (any number of bands, Linkwitz-Riley, summing flat), the biquad, the audited float↔integer conversions, the click statistic (stereo or mono) and averaging a stereo input to a DI |
| `family-signals` | Test support, for `[dev-dependencies]`: the synthetic plucks, clicks, notes, sines and seeded noise the plugins' tests play, and their RMS and peak levels |
| `family-ui` | The editor look, Night (`docs/family-identity.md`), in egui on nice-plug: the dial, switch, lamp and glow, the bundled Archivo, the look's colours and sizes, and the evenly scaled canvas with its corner grip |
| `family-wav` | WAV reading and writing for each plugin's offline `render`, and the `stats`, `split` and `join` commands (also runnable on their own as the `wav` binary) |
| `family-xtask` | `cargo xtask bundle` and `cargo xtask install`: each plugin's `xtask` is a `main` that calls `family_xtask::main` with its package, bundle name and install folder variable |

## Using it from a plugin

Plugins depend on it by git rev, like nih-plug:

```toml
family-dsp = { git = "https://github.com/iuri-audio/family", rev = "<commit>" }
```

A change here reaches a plugin when that plugin bumps the rev. To work on both at once, with this repo checked out beside the plugin's, build the plugin against this checkout with `.cargo/local.toml`:

```sh
cargo test --workspace --config ../family/.cargo/local.toml
```

While patched, Cargo rewrites the plugin's `Cargo.lock` to point at the local paths; the next build without `--config` points it back at the pinned rev. Push the change here and bump the plugin's rev before committing the plugin.

## Rules

The same as the plugins': `cargo +nightly fmt --all` before every commit, clippy with the workspace lints, and `family-dsp` is `no_std` without `alloc` so nothing in it can allocate on the audio thread. Nothing here may know which plugin it's in: anything plugin-specific is passed in.
