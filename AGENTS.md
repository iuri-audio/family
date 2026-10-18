# family

Code shared by the family's guitar plugins (Doubler, Quiet, Drop). See `README.md` for the crates and how plugins use them.

- Run `cargo +nightly fmt --all` before every commit; CI checks it with clippy and the tests.
- Nothing here may name or assume a particular plugin: take it as a parameter.
- The editor look and its tokens are in `docs/family-identity.md`; `family-ui` implements it.
- Upstream (nice-plug, egui-baseview, baseview): `docs/upstream.md` has the bugs we work around, our baseview fork, and who reports upstream.
- `family-dsp` is `no_std` without `alloc` (each plugin's ADR on its dsp crate explains why).
- A change here only reaches a plugin when the plugin bumps its rev. When changing an interface, update both plugins in the same session.
