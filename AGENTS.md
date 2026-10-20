# family

Code shared by the family's guitar plugins (Doubler, Quiet, Drop). See `README.md` for the crates and how plugins use them.

- Run `cargo +nightly fmt --all` before every commit; CI checks it with clippy and the tests.
- Nothing here may name or assume a particular plugin: take it as a parameter.
- The editor look and its tokens are in `docs/family-identity.md`; `family-ui` implements it.
- A bug only a host shows (the editor, automation, transport): reproduce it in Reaper yourself with a ReaScript, following `docs/driving-reaper.md`.
- Before a plugin bumps its family rev, run `cargo xtask e2e` there (`docs/driving-reaper.md`, *The end-to-end checks*), with `--config ../family/.cargo/local.toml` to try a family change. Test hooks go only in `crates/ui/src/e2e.rs`, behind the `e2e-hooks` feature.
- Upstream (nice-plug, egui-baseview, baseview): `docs/upstream.md` has the bugs we work around, our baseview fork, and who reports upstream.
- `family-dsp` is `no_std` without `alloc` (Doubler's ADR 0004 and Quiet's ADR 0002 explain why).
- A change here only reaches a plugin when the plugin bumps its rev. When changing an interface, update every plugin that uses it (Doubler, Drop, Quiet) in the same session.
