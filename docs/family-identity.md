# The plugin family's look

Doubler was the first of a set of guitar plugins; Quiet (noise reduction) is the second and a pitch shifter is planned. They share one look, called **Night**, chosen in Doubler's #21. This page is what each plugin starts from.

## What the family shares

- **The panel:** dark graphite, the same in every plugin.
- **The typeface:** Archivo, bundled, at one width in two weights (regular for values and labels, semi-bold for names), with tabular figures so numbers don't jitter.
- **The components:** the dial, the switch and the lamp, drawn the same way everywhere.
- **One colour per plugin.** Each plugin owns a signal colour and uses it for the same things: the value arc, the lamp, the switch when on, and a bar along the top edge. Nothing else is coloured. The plugin's name stays in the ink colour.
- **Glow on what's lit.** The value arc and a lit lamp glow in the plugin's colour, like LEDs behind smoked plastic.

The layout is not shared. Doubler's one large dial suits a plugin with one main control; the next plugin's layout follows its own controls (a semitone display, a gate meter). Different layouts also help tell the plugins apart.

## The tokens

| Role | Value |
| --- | --- |
| Panel | `#171b20` |
| Ink (names, values) | `#e9edf0` |
| Muted (units, end labels, secondary values) | `#86919c` |
| Rule between areas | `#262c33` |
| Dial track, switch track when off | `#262d35` |
| Lamp when dark | `#2e353d` |
| Switch knob when off | `#86919c` |
| Dial end dot | `#ffffff` |
| Colour bar | 3 px, the plugin's colour |

Each plugin's colour comes in three strengths: the colour itself (arc, switch, bar), a lighter tint for the lit lamp, and the colour at reduced opacity for the glow.

| Plugin | Colour | Lit lamp | Glow |
| --- | --- | --- | --- |
| Doubler | `#57cfbd` | `#8af0e0` | `#57cfbd8c` on the arc, `#57cfbdcc` on the lamp |
| Pitch shifter (proposed) | `#f0a43a` | `#ffc56e` | same opacities |
| Quiet | `#a396ff` | `#c4bbff` | same opacities |

The glows are `night::DIAL_GLOW` on a lit dial's arc (a sigma of 4.5 points) and `night::LAMP_GLOW` on the lamp (a sigma of 2.5 points, the lamp grown by 3 points first). Their widths were chosen by eye to match the vizia build's `box-shadow`s (`0px 0px 18px` and `0px 0px 10px 3px`), which femtovg rendered at a sigma of about a quarter of the blur radius.

## The shared code

`family-ui` in this repo holds what the plugins share, in egui on nice-plug:

- `night`: the tokens above as Rust constants, with the text sizes, the colour bar's height, the lamp's and switch's sizes and the two glows. A plugin's colour is a `Colour { signal, lit }`.
- `text`: `install(ctx)` loads the bundled Archivo as egui's proportional font, with semi-bold as a named family; `font(size, Weight)` picks a weight. Call `install` from the editor's `build`.
- `Canvas`: the editor is painted at fixed coordinates on a canvas of the plugin's design size, and egui's zoom (the host's display scale times the user's scale) scales it evenly. It draws the panel, the colour bar and the corner grip, which changes the user's scale live. It also carries the workarounds for egui-baseview's and nice-plug-egui's bugs, each commented with the bug it covers.
- `EditorScale`: the user's scale, which the plugin saves as a persistent field, in the form vizia's `ViziaState` was saved in.
- `Dial`: the 270° dial, with drag, fine drag, double-click reset, wheel steps and host gestures. The plugin places and sizes it; `glowing()` lights its arc.
- `Switch`: the on/off switch, followed by the parameter's name.
- `Lamp`: the lamp that blinks when a counter the audio thread keeps moves.
- `glow`: the shapes as meshes whose vertex alphas follow a cross-section, crisp or blurred. A glow is the shape's cross-section blurred by a Gaussian, worked out analytically, so it needs no texture or blur pass and scales with the editor.

Each takes the plugin's `Colour` and knows nothing else about the plugin it's in. The layout is each plugin's own `editor.rs`.
