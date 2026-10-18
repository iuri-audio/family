# Upstream

The editor stack family-ui runs on, the upstream bugs we work around, and how upstream gets told about them.

## The stack

nice-plug-egui 0.5 → egui-baseview 0.7 → baseview 0.3, all from [RustAudio](https://codeberg.org/RustAudio/nice-plug). egui-baseview 0.8 and later are on baseview 0.4; nice-plug-egui moves there with [nice-plug#97](https://codeberg.org/RustAudio/nice-plug/pulls/97). Once a release has it, revisit everything below (doubler tracks this in [doubler#35](https://github.com/iuri-audio/doubler/issues/35)).

## Workarounds in family-ui

`Canvas` (`crates/ui/src/canvas.rs`) works around four egui-baseview 0.7 and nice-plug-egui 0.5 bugs, listed in its module docs and commented where each is handled. Drop each workaround once its bug is fixed upstream.

## The baseview fork

baseview 0.3.7's X11 backend can leave an editor window blank for good: it decides whether to draw from a list of the window's ancestors, which can miss an ancestor being mapped. In Reaper under XWayland about every other opening stayed blank. [`iuri-audio/baseview@fix/x11-missed-ancestor-map`](https://github.com/iuri-audio/baseview/tree/fix/x11-missed-ancestor-map) fixes it on top of 0.3.7, and each plugin on egui patches crates.io's `baseview` to it in its workspace `Cargo.toml`. baseview 0.4 no longer tracks ancestors, so the fork goes with the move to it.

## Reporting upstream

RustAudio's projects follow the [RustAudio AI policy](https://rust.audio/community/ai/): no AI-written pull requests, descriptions or comments. Issues, pull requests and comments on baseview, egui-baseview and nice-plug are the developer's to write. An agent gathers the facts for them and fixes things in our own forks and repos.
