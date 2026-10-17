//! The evenly scaled canvas a plugin lays its editor out on, with its corner grip.
//!
//! A plugin draws its whole editor at fixed coordinates on a canvas of its design size, in points.
//! egui's zoom scales it evenly, so the layout never reflows: the zoom is the host's display scale
//! times the user's [`EditorScale`], which the corner grip changes live and the plugin saves.
//!
//! Most of this works around bugs in egui-baseview 0.7 and nice-plug-egui 0.5, each commented
//! where it's handled so it can go once fixed upstream (`UPSTREAM-CONTRIBUTIONS.md` beside the
//! repos has the details):
//!
//! 1. nice-plug-egui never calls `NiceEguiApp::editor_closed`, so per-window state is reset in
//!    `build` instead, through [`Canvas::opened`].
//! 2. egui-baseview converts mouse events with the zoom the previous frame ran at.
//! 3. egui-baseview's window size drifts with each zoom change.
//! 4. egui's scale can differ from the host's: the first editor window in a process misses the
//!    host's display scale on Windows, and on Linux Reaper doesn't scale its windows by the display
//!    scale egui reads.
//!
//! It logs what it measures and every zoom and size it asks for through nice-plug's logger, so
//! `NICE_LOG` set to a file path shows how an editor settled in a host.

use std::sync::Arc;

use egui::{Event, Id, Pos2, Rect, Sense, Ui, Vec2, ViewportCommand, pos2, vec2};
use nice_plug::debug::nice_log;

use crate::Colour;
use crate::night::{COLOUR_BAR_HEIGHT, MUTED, PANEL};
use crate::scale::{self, EditorScale};

/// The corner grip's size.
const GRIP_SIZE: f32 = 16.0;

/// How long the grip waits for the host to resize the window to its last zoom before stepping the
/// zoom again, in seconds, so a host that refuses or rounds a size can't stall it.
const GRIP_PATIENCE: f64 = 0.2;

/// How many frames an opening editor waits for its window to take the canvas's shape before it
/// gives up measuring the host's scale. Reaper first shows a 10000-pixel square placeholder.
const OPENING_FRAMES: u32 = 30;

/// The evenly scaled canvas, kept by the plugin's editor across openings.
#[derive(Debug)]
pub struct Canvas {
    /// The design size, in points.
    size: Vec2,
    /// The user's scale, saved by the plugin.
    scale: Arc<EditorScale>,
    /// The plugin's colour, for the bar along the top edge.
    colour: Colour,
    /// The host's display scale that egui's own pixels-per-point misses, measured from the window
    /// on opening (bug 4): the host's whole scale for the first window in a process, 1 once egui
    /// gets it right.
    missed_scale: f32,
    /// Frames since the editor opened, while the missed scale is yet to be measured.
    opening: Option<u32>,
    /// Whether a zoom has been set that is yet to take effect.
    settling: bool,
    /// The zoom the previous frame ran at, which egui-baseview converts this frame's mouse events
    /// with (bug 2).
    last_zoom: f32,
    /// A wrong window size last asked to be corrected, so a host that refuses isn't asked every
    /// frame.
    corrected: Option<Vec2>,
    /// While the grip is dragged: the zoom and the pointer, in pixels, when it started.
    grip: Option<(f32, Pos2)>,
    /// When the grip last set the zoom, in egui's time in seconds.
    zoom_set_at: f64,
}

impl Canvas {
    /// A canvas of `size` points, scaled by `scale`, with the bar along its top in `colour`.
    #[must_use]
    pub const fn new(size: Vec2, scale: Arc<EditorScale>, colour: Colour) -> Self {
        Self {
            size,
            scale,
            colour,
            missed_scale: 1.0,
            opening: Some(0),
            settling: false,
            last_zoom: 1.0,
            corrected: None,
            grip: None,
            zoom_set_at: 0.0,
        }
    }

    /// Starts measuring the host's scale afresh for a newly opened window. Call it from the
    /// editor's `build`, since `editor_closed` is never called (bug 1).
    pub const fn opened(&mut self) {
        self.missed_scale = 1.0;
        self.opening = Some(0);
        self.settling = false;
        self.corrected = None;
        self.grip = None;
    }

    /// Scales the canvas and fills it with the panel, then has `contents` draw the editor on it
    /// at fixed coordinates in points, then draws the bar along the top and the corner grip over
    /// it. Until the scale has settled, it shows only the panel.
    pub fn show(&mut self, ui: &mut Ui, contents: impl FnOnce(&mut Ui)) {
        let ctx = ui.ctx().clone();
        let zoom = ctx.zoom_factor();
        let points = ctx.content_rect().size();

        // egui-baseview resizes the window to follow a new zoom from its current size in points,
        // so rounding drifts (bug 3). In the frame a new zoom takes effect, pin the window to
        // exactly the canvas at it.
        if zoom.to_bits() != self.last_zoom.to_bits() {
            nice_log!(
                "family-ui: zoom {} took effect, pinning the window to the canvas",
                zoom
            );
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(self.size));
            self.settling = false;
        }
        if let Some(frames) = self.opening {
            self.measure(&ctx, points, zoom, frames);
        } else if self.grip.is_none()
            && (points - self.size).length() > 1.0
            && self.corrected != Some(points)
        {
            self.corrected = Some(points);
            nice_log!(
                "family-ui: window is {:?} points, not the canvas's {:?}; asking for the canvas's",
                points,
                self.size
            );
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(self.size));
        }

        ui.painter().rect_filled(ctx.content_rect(), 0.0, PANEL);
        if self.opening.is_none() && !self.settling {
            contents(ui);
            ui.painter().rect_filled(
                Rect::from_min_size(Pos2::ZERO, vec2(self.size.x, COLOUR_BAR_HEIGHT)),
                0.0,
                self.colour.signal,
            );
            self.grip(ui, zoom);
        }
        self.last_zoom = zoom;
    }

    /// One frame of measuring the host's scale on opening, `frames` in, with the window `points`
    /// in size at `zoom`.
    ///
    /// The host sizes the window with its display scale, but egui's scale can differ from it (bug
    /// 4), so egui sees the window that much too big or too small in points. That ratio is the
    /// scale it missed. The zoom is that times the user's scale, which the plugin keeps itself:
    /// nice-plug-egui's own saved zoom would count the missed scale twice.
    fn measure(&mut self, ctx: &egui::Context, points: Vec2, zoom: f32, frames: u32) {
        let measured = missed_scale(points, zoom, self.size);
        if measured.is_none() && frames < OPENING_FRAMES {
            self.opening = Some(frames + 1);
            return;
        }
        self.opening = None;
        self.missed_scale = measured.unwrap_or(1.0);
        let target = self.missed_scale * self.scale.get();
        nice_log!(
            "family-ui: opened at {:?} points and zoom {} after {} frames; missed scale {:?}, \
             zoom {}",
            points,
            zoom,
            frames,
            measured,
            target
        );
        if (target - zoom).abs() > 1e-3 {
            ctx.set_zoom_factor(target);
            self.settling = true;
        } else if (points - self.size).length() > 1.0 {
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(self.size));
        }
    }

    /// The corner grip: dragging it changes the zoom, and egui-baseview resizes the window to
    /// follow.
    fn grip(&mut self, ui: &Ui, zoom: f32) {
        let ctx = ui.ctx();
        let rect = Rect::from_min_max(
            self.size.to_pos2() - Vec2::splat(GRIP_SIZE),
            self.size.to_pos2(),
        );
        let response = ui.interact(rect, Id::new("family-ui grip"), Sense::drag());
        // Pointer positions in pixels, which don't change as the zoom follows the size.
        if response.drag_started() {
            self.grip = ctx
                .input(|input| input.pointer.interact_pos())
                .map(|pointer| (zoom, (pointer.to_vec2() * zoom).to_pos2()));
        }
        // Only this frame's own mouse moves, converted back to pixels with the zoom egui-baseview
        // converted them with: the previous frame's (bug 2). Rescaling with this frame's swings a
        // still mouse back and forth while the zoom changes.
        let event_zoom = self.last_zoom;
        let moved = ctx.input(|input| {
            input
                .raw
                .events
                .iter()
                .rev()
                .find_map(|event| match *event {
                    Event::PointerMoved(pointer) => {
                        Some((pointer.to_vec2() * event_zoom).to_pos2())
                    }
                    _ => None,
                })
        });
        if let (Some((from_zoom, from)), Some(pointer)) = (self.grip, moved)
            && response.dragged()
        {
            // Until the host has resized the window to the last zoom, the frame is drawn cropped:
            // wait for it before stepping the zoom again. Whole-pixel window sizes leave it a
            // point or so off, so within 1 % counts.
            let now = ctx.input(|input| input.time);
            let off = (ctx.content_rect().size() - self.size).abs();
            let caught_up = (off.x <= self.size.x * 0.01 && off.y <= self.size.y * 0.01)
                || now - self.zoom_set_at > GRIP_PATIENCE;
            let new_zoom = (from_zoom * grip_scale(pointer - from, from_zoom, self.size)).clamp(
                scale::MIN * self.missed_scale,
                scale::MAX * self.missed_scale,
            );
            if caught_up {
                self.scale.set(new_zoom / self.missed_scale);
                if (new_zoom - zoom).abs() > 1e-3 {
                    nice_log!("family-ui: grip sets zoom {}", new_zoom);
                    ctx.set_zoom_factor(new_zoom);
                    self.zoom_set_at = now;
                }
            }
        }
        if response.drag_stopped() {
            self.grip = None;
        }

        let corner = rect.max - Vec2::splat(3.0);
        for reach in [4.0, 8.0, 12.0] {
            ui.painter().line_segment(
                [
                    pos2(corner.x - reach, corner.y),
                    pos2(corner.x, corner.y - reach),
                ],
                (1.0, MUTED),
            );
        }
    }
}

/// The host's display scale egui missed, from a window `points` in size in egui's points at `zoom`
/// showing a canvas of `size`: the ratio of the window at a zoom of 1 to the canvas, if the window
/// has the canvas's shape. `None` while it doesn't, such as while a host shows a placeholder.
///
/// nice-plug-egui opens each window at the zoom the last one closed at, so the zoom on opening
/// isn't always 1.
fn missed_scale(points: Vec2, zoom: f32, size: Vec2) -> Option<f32> {
    let same_shape = (points.y / points.x - size.y / size.x).abs() < 0.01;
    same_shape.then(|| points.x * zoom / size.x)
}

/// How much a grip drag of `drag` pixels scales a canvas of `size` points shown at `zoom`: by the
/// width it would have if its bottom right corner followed the pointer, along whichever of the two
/// edges the drag moves further, so the canvas keeps its shape.
fn grip_scale(drag: Vec2, zoom: f32, size: Vec2) -> f32 {
    let width = size.x * zoom;
    let grown = drag.x.max(drag.y * size.x / size.y);
    (width + grown) / width
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: Vec2 = vec2(720.0, 420.0);

    #[test]
    fn a_window_of_the_canvas_shape_gives_the_missed_scale() {
        let measured = missed_scale(SIZE * 1.5, 1.0, SIZE).expect("same shape");

        assert!((measured - 1.5).abs() < 1e-6);
    }

    #[test]
    fn a_window_opened_at_the_last_zoom_gives_the_same_missed_scale() {
        // Reaper on Linux: egui's scale is 1.6458 but the window stays 720×420 pixels. The first
        // window opens at a zoom of 1; nice-plug-egui opens the next at the zoom it settled on.
        let first = missed_scale(SIZE / 1.645_833_4, 1.0, SIZE).expect("same shape");
        let next = missed_scale(SIZE, first, SIZE).expect("same shape");

        assert!((first - 0.607_595_5).abs() < 1e-6);
        assert!((next - first).abs() < 1e-6);
    }

    #[test]
    fn a_window_of_another_shape_gives_no_scale() {
        assert_eq!(missed_scale(vec2(10_000.0, 10_000.0), 1.0, SIZE), None);
    }

    #[test]
    fn dragging_the_grip_by_the_canvas_width_doubles_it() {
        let scale = grip_scale(vec2(720.0 * 1.5, 0.0), 1.5, SIZE);

        assert!((scale - 2.0).abs() < 1e-6);
    }

    #[test]
    fn the_grip_follows_whichever_edge_the_drag_moves_further() {
        // 42 pixels down is a tenth of the height, more than 10 pixels right is of the width.
        let scale = grip_scale(vec2(10.0, 42.0), 1.0, SIZE);

        assert!((scale - 1.1).abs() < 1e-6);
    }

    #[test]
    fn dragging_the_grip_inward_shrinks_the_canvas() {
        let scale = grip_scale(vec2(-72.0, -42.0), 1.0, SIZE);

        assert!((scale - 0.9).abs() < 1e-6);
    }
}
