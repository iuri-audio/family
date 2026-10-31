//! The evenly scaled canvas a plugin lays its editor out on, with its corner grip.
//!
//! A plugin draws its whole editor at fixed coordinates on a canvas of its design size, in points.
//! egui's zoom scales it evenly, so the layout never reflows: the zoom is the host's display scale
//! times the user's [`EditorScale`], which the corner grip changes live and the plugin saves.
//!
//! Most of this works around bugs in egui-baseview 0.7 and nice-plug-egui 0.5, each commented
//! where it's handled so it can go once fixed upstream (`docs/upstream.md` says when to revisit
//! them):
//!
//! 1. nice-plug-egui never calls `NiceEguiApp::editor_closed`, so per-window state is reset in
//!    `build` instead, through [`Canvas::opened`].
//! 2. egui-baseview converts mouse events with the zoom the previous frame ran at.
//! 3. egui-baseview's window size drifts with each zoom change.
//! 4. egui's scale can differ from the host's: the first editor window in a process misses the
//!    host's display scale on Windows, and on Linux Reaper doesn't scale its windows by the display
//!    scale egui reads.
//! 5. nice-plug-egui can retain a size that already includes zoom, which egui-baseview multiplies
//!    by zoom again on reopening. Reuse the first window's scale calibration instead of measuring
//!    that enlarged window as another display-scale change.
//! 6. On Windows, a child already at the desired size emits no `WM_SIZE` when pinned to that size,
//!    so a smaller host parent can stay cropped. Make an actual size change on opening, then
//!    restore the exact canvas size to notify the host.
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

/// Opening resize steps. A real size change makes baseview notify the host even when the child
/// already has the desired size and resizing it to that same size would emit no event.
#[derive(Debug, Clone, Copy)]
enum HostResize {
    Notify,
    Restore,
}

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
    /// egui's native scale when the host's scale was calibrated. Kept across openings so a new
    /// native scale can be accounted for without measuring an already zoomed opening size.
    native_scale: Option<f32>,
    /// Synchronise the host's opening size with the child window, then restore the exact size.
    host_resize: Option<HostResize>,
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
    /// The latest grip request, retained until the host catches up, even after release.
    pending_zoom: Option<f32>,
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
            native_scale: None,
            host_resize: if cfg!(target_os = "windows") {
                Some(HostResize::Notify)
            } else {
                None
            },
            opening: Some(0),
            settling: false,
            last_zoom: 1.0,
            corrected: None,
            grip: None,
            zoom_set_at: 0.0,
            pending_zoom: None,
        }
    }

    /// Starts settling a newly opened window. Call it from the
    /// editor's `build`, since `editor_closed` is never called (bug 1).
    ///
    /// It logs the opening before the window draws anything, so an editor that never draws shows
    /// in the log as an opening with no "opened at" line after it.
    pub fn opened(&mut self) {
        nice_log!("family-ui: editor window built, waiting for its first frame");
        self.opening = Some(0);
        self.settling = false;
        self.corrected = None;
        self.grip = None;
        self.pending_zoom = None;
        self.host_resize = if cfg!(target_os = "windows") {
            Some(HostResize::Notify)
        } else {
            None
        };
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

        if self.opening.is_none() && !self.settling {
            match self.host_resize {
                Some(HostResize::Notify) => {
                    // baseview only notifies the host on a changed WM_SIZE. On Windows the
                    // child can already be at the desired size while its parent is still too
                    // small, so pinning it to that same size leaves the host cropped. Nudge it
                    // once, then restore it next frame. Two points still change a whole physical
                    // pixel at the smallest user scale. Keep the panel blank until this settles.
                    ctx.send_viewport_cmd(ViewportCommand::InnerSize(self.size + Vec2::splat(2.0)));
                    self.host_resize = Some(HostResize::Restore);
                    ctx.request_repaint();
                }
                Some(HostResize::Restore) => {
                    ctx.send_viewport_cmd(ViewportCommand::InnerSize(self.size));
                    self.host_resize = None;
                    ctx.request_repaint();
                }
                None => {}
            }
        }

        ui.painter().rect_filled(ctx.content_rect(), 0.0, PANEL);
        if self.opening.is_none() && !self.settling && self.host_resize.is_none() {
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
        let native = ctx.native_pixels_per_point().unwrap_or(1.0);
        // A reopened window is not a fresh display-scale measurement: nice-plug-egui may have
        // saved its zoomed size without receiving a resize event that strips the zoom (bug 5).
        // Keep the first calibration, compensating if egui now accounts for more of the scale.
        self.missed_scale = self.native_scale.map_or_else(
            || measured.unwrap_or(1.0),
            |previous| self.missed_scale * previous / native,
        );
        if measured.is_some() || self.native_scale.is_some() {
            self.native_scale = Some(native);
        }
        let target = self.missed_scale * self.scale.get();
        nice_log!(
            "family-ui: opened at {:?} points and zoom {} after {} frames; missed scale {:?}, \
             zoom {}",
            points,
            zoom,
            frames,
            self.missed_scale,
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
            && (response.dragged() || response.drag_stopped())
        {
            self.pending_zoom = Some(
                (from_zoom * grip_scale(pointer - from, from_zoom, self.size)).clamp(
                    scale::MIN * self.missed_scale,
                    scale::MAX * self.missed_scale,
                ),
            );
        }
        if let Some(new_zoom) = self.pending_zoom {
            // Until the host has resized the window to the last zoom, the frame is drawn cropped:
            // wait for it before stepping the zoom again. Whole-pixel window sizes leave it a
            // point or so off, so within 1 % counts. Keep the request across frames: waiting must
            // not discard the final mouse move, including one delivered with a button release.
            let now = ctx.input(|input| input.time);
            let off = (ctx.content_rect().size() - self.size).abs();
            let caught_up = (off.x <= self.size.x * 0.01 && off.y <= self.size.y * 0.01)
                || now - self.zoom_set_at >= GRIP_PATIENCE;
            if caught_up {
                self.pending_zoom = None;
                self.scale.set(new_zoom / self.missed_scale);
                if (new_zoom - zoom).abs() > 1e-3 {
                    nice_log!("family-ui: grip sets zoom {}", new_zoom);
                    ctx.set_zoom_factor(new_zoom);
                    self.zoom_set_at = now;
                }
            } else {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                    (self.zoom_set_at + GRIP_PATIENCE - now).max(0.0),
                ));
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
    fn opening_notifies_a_host_that_only_hears_actual_child_size_changes() {
        let ctx = egui::Context::default();
        ctx.options_mut(|options| options.zoom_factor = 1.5);
        let mut canvas = Canvas::new(
            SIZE,
            Arc::new(EditorScale::default()),
            Colour {
                signal: egui::Color32::WHITE,
                lit: egui::Color32::WHITE,
            },
        );
        // The Windows child has its final physical size, but Reaper's parent is still at
        // the unscaled size. No WM_SIZE (and therefore no host request) is emitted if a
        // viewport command asks for exactly the size the child already has.
        canvas.host_resize = Some(HostResize::Notify);
        let mut child = SIZE * 1.5;
        let mut host = SIZE;
        for _ in 0..4 {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, child / ctx.zoom_factor())),
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| canvas.show(ui, |_| {}));
            output.textures_delta.clear();
            for viewport in output.viewport_output.values() {
                for command in &viewport.commands {
                    if let ViewportCommand::InnerSize(size) = command {
                        let requested = (*size * ctx.zoom_factor()).round();
                        if requested != child {
                            child = requested;
                            host = requested;
                        }
                    }
                }
            }
        }
        assert_eq!(child, SIZE * 1.5);
        assert_eq!(host, child, "the parent clips the correctly scaled child");
    }

    #[test]
    fn reopening_at_windows_display_scale_keeps_the_same_zoom() {
        let scale = Arc::new(EditorScale::default());
        let mut canvas = Canvas::new(
            SIZE,
            Arc::clone(&scale),
            Colour {
                signal: egui::Color32::WHITE,
                lit: egui::Color32::WHITE,
            },
        );
        let mut restored_zoom = 1.0;
        for opening in 0..4 {
            let ctx = egui::Context::default();
            ctx.set_zoom_factor(restored_zoom);
            canvas.opened();
            // egui-baseview prepares the first input using the restored zoom before egui
            // has applied it. Windows supplies the real window size immediately.
            frame(&mut canvas, &ctx, 0.0, Vec2::splat(10_000.0), vec![]);
            // The adapter can retain a size that already includes the previous zoom when
            // Windows sends no resize event after pinning the window to its existing size.
            let window = SIZE * 1.5 * restored_zoom;
            frame(&mut canvas, &ctx, 0.01, window, vec![]);
            frame(&mut canvas, &ctx, 0.02, window, vec![]);
            restored_zoom = ctx.zoom_factor();
            assert!(
                (restored_zoom - 1.5).abs() < 1e-5,
                "opening {opening}: expected zoom 1.5, got {restored_zoom}"
            );
            assert!((scale.get() - 1.0).abs() < 1e-5);
        }
    }

    fn frame(
        canvas: &mut Canvas,
        ctx: &egui::Context,
        time: f64,
        window: Vec2,
        events: Vec<Event>,
    ) -> Frame {
        frame_with_native(canvas, ctx, time, window, events, 1.0)
    }

    /// What one frame did.
    struct Frame {
        /// Whether the editor's contents were drawn.
        drew: bool,
        /// The window sizes it asked for.
        requested: Vec<Vec2>,
        /// When it asked to be drawn again.
        repaint: std::time::Duration,
        /// The lines it painted: the grip's.
        lines: Vec<[Pos2; 2]>,
    }

    fn frame_with_native(
        canvas: &mut Canvas,
        ctx: &egui::Context,
        time: f64,
        window: Vec2,
        events: Vec<Event>,
        native: f32,
    ) -> Frame {
        let mut input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                Pos2::ZERO,
                window / (ctx.zoom_factor() * native),
            )),
            time: Some(time),
            events,
            // egui takes this off a requested delay, and tests read the delay.
            predicted_dt: 0.0,
            ..Default::default()
        };
        input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(native);
        let mut drew = false;
        let mut output = ctx.run_ui(input, |ui| canvas.show(ui, |_| drew = true));
        output.textures_delta.clear();
        let viewport = &output.viewport_output[&egui::ViewportId::ROOT];
        Frame {
            drew,
            requested: viewport
                .commands
                .iter()
                .filter_map(|command| match command {
                    ViewportCommand::InnerSize(size) => Some(*size),
                    _ => None,
                })
                .collect(),
            repaint: viewport.repaint_delay,
            lines: output
                .shapes
                .iter()
                .filter_map(|shape| match shape.shape {
                    egui::Shape::LineSegment { points, .. } => Some(points),
                    _ => None,
                })
                .collect(),
        }
    }

    struct Rig {
        ctx: egui::Context,
        canvas: Canvas,
        scale: Arc<EditorScale>,
    }

    /// An open canvas in a window `host_scale` times its size, settled, with no host resize
    /// pending whatever the platform, at a time of 10 seconds.
    fn settled(host_scale: f32) -> Rig {
        let ctx = egui::Context::default();
        let scale = Arc::new(EditorScale::default());
        let mut canvas = Canvas::new(
            SIZE,
            Arc::clone(&scale),
            Colour {
                signal: egui::Color32::WHITE,
                lit: egui::Color32::WHITE,
            },
        );
        canvas.host_resize = None;
        for time in [9.0, 9.1, 9.2, 10.0] {
            frame(&mut canvas, &ctx, time, SIZE * host_scale, vec![]);
        }
        assert!((ctx.zoom_factor() - host_scale).abs() < 1e-5);
        Rig { ctx, canvas, scale }
    }

    const GRIP: Pos2 = pos2(712.0, 412.0);

    fn press(at: Pos2) -> Vec<Event> {
        vec![
            Event::PointerMoved(at),
            Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            },
        ]
    }

    impl Rig {
        /// The zoom after another frame, which is when egui applies a new one.
        fn zoom_next_frame(&mut self) -> f32 {
            frame(&mut self.canvas, &self.ctx, 10.03, SIZE, vec![]);
            self.ctx.zoom_factor()
        }

        /// Presses the grip at 10.01 s, and drags it to `to` in points at 10.02 s.
        fn drag_grip(&mut self, host_scale: f32, to: Pos2) {
            let window = SIZE * host_scale;
            frame(&mut self.canvas, &self.ctx, 10.01, window, press(GRIP));
            frame(
                &mut self.canvas,
                &self.ctx,
                10.02,
                window,
                vec![Event::PointerMoved(to)],
            );
        }
    }

    #[test]
    fn a_new_zoom_pins_the_window_to_the_canvas_only_as_it_takes_effect() {
        let mut rig = settled(1.0);
        rig.drag_grip(1.0, GRIP + vec2(72.0, 0.0));

        // egui applies a new zoom in the next frame, and works out the window's size itself.
        let takes_effect = frame(&mut rig.canvas, &rig.ctx, 10.03, SIZE, vec![]);
        assert_eq!(takes_effect.requested, [SIZE]);

        let after = frame(&mut rig.canvas, &rig.ctx, 10.04, SIZE * 1.1, vec![]);
        assert_eq!(after.requested, []);
    }

    #[test]
    fn a_window_that_isnt_the_canvass_size_is_corrected_once_per_size() {
        let mut rig = settled(1.0);
        let wide = SIZE + vec2(50.0, 0.0);
        let tall = SIZE + vec2(0.0, 50.0);

        for (time, window, expected) in [
            (10.1, SIZE, vec![]),
            (10.2, wide, vec![SIZE]),
            (10.3, wide, vec![]),
            (10.4, tall, vec![SIZE]),
            (10.5, tall, vec![]),
            (10.6, SIZE, vec![]),
        ] {
            let requested = frame(&mut rig.canvas, &rig.ctx, time, window, vec![]).requested;
            assert_eq!(requested, expected, "at {time}");
        }
    }

    #[test]
    fn a_window_following_the_grip_isnt_corrected_during_the_drag() {
        let mut rig = settled(1.0);
        frame(&mut rig.canvas, &rig.ctx, 10.01, SIZE, press(GRIP));
        frame(
            &mut rig.canvas,
            &rig.ctx,
            10.02,
            SIZE + vec2(50.0, 0.0),
            vec![],
        );

        let during = frame(
            &mut rig.canvas,
            &rig.ctx,
            10.03,
            SIZE + vec2(50.0, 0.0),
            vec![],
        );

        assert_eq!(during.requested, []);
    }

    #[test]
    fn a_host_that_only_hears_size_changes_gets_one_nudge_with_the_editor_blank_meanwhile() {
        let mut rig = settled(1.0);
        rig.canvas.host_resize = Some(HostResize::Notify);

        let nudged = frame(&mut rig.canvas, &rig.ctx, 10.1, SIZE, vec![]);
        assert_eq!(nudged.requested, [SIZE + Vec2::splat(2.0)]);
        assert!(!nudged.drew);

        let restored = frame(&mut rig.canvas, &rig.ctx, 10.2, SIZE, vec![]);
        assert_eq!(restored.requested, [SIZE]);
        assert!(restored.drew);

        let after = frame(&mut rig.canvas, &rig.ctx, 10.3, SIZE, vec![]);
        assert_eq!(after.requested, []);
        assert!(after.drew);
    }

    #[test]
    fn a_host_is_only_nudged_once_the_scale_has_settled() {
        let ctx = egui::Context::default();
        let mut canvas = Canvas::new(
            SIZE,
            Arc::new(EditorScale::default()),
            Colour {
                signal: egui::Color32::WHITE,
                lit: egui::Color32::WHITE,
            },
        );
        canvas.host_resize = Some(HostResize::Notify);
        let placeholder = Vec2::splat(10_000.0);

        // Still opening: a placeholder window can't be measured.
        let opening = frame(&mut canvas, &ctx, 0.0, placeholder, vec![]);
        assert_eq!(opening.requested, []);
        assert!(!opening.drew);

        // Measured, with a new zoom on its way.
        let window = SIZE * 1.5;
        let measured = frame(&mut canvas, &ctx, 0.1, window, vec![]);
        assert_eq!(measured.requested, []);
        assert!(!measured.drew);
    }

    #[test]
    fn a_window_that_never_gets_the_canvass_shape_stops_blocking_the_editor() {
        let ctx = egui::Context::default();
        let mut canvas = Canvas::new(
            SIZE,
            Arc::new(EditorScale::default()),
            Colour {
                signal: egui::Color32::WHITE,
                lit: egui::Color32::WHITE,
            },
        );
        canvas.host_resize = None;
        let window = Vec2::splat(500.0);

        for index in 0..OPENING_FRAMES {
            let opening = frame(&mut canvas, &ctx, f64::from(index) / 60.0, window, vec![]);
            assert!(!opening.drew, "frame {index}");
            assert_eq!(opening.requested, [], "frame {index}");
        }

        // It gives up, shows the editor at the user's scale, and asks for the right window.
        let gave_up = frame(&mut canvas, &ctx, 1.0, window, vec![]);
        assert!(gave_up.drew);
        assert_eq!(gave_up.requested, [SIZE]);
    }

    #[test]
    fn a_window_opening_at_the_canvass_size_needs_no_correction() {
        let ctx = egui::Context::default();
        let mut canvas = Canvas::new(
            SIZE,
            Arc::new(EditorScale::default()),
            Colour {
                signal: egui::Color32::WHITE,
                lit: egui::Color32::WHITE,
            },
        );
        canvas.host_resize = None;

        let opened = frame(&mut canvas, &ctx, 0.0, SIZE, vec![]);

        assert!(opened.drew);
        assert_eq!(opened.requested, []);
    }

    #[test]
    fn the_grip_follows_the_pointer_in_pixels_on_a_scaled_display() {
        let mut rig = settled(1.5);

        // 72 points at a zoom of 1.5 is 108 pixels: a tenth of the 1080-pixel-wide canvas.
        rig.drag_grip(1.5, GRIP + vec2(72.0, 0.0));

        assert!((rig.scale.get() - 1.1).abs() < 1e-4, "{}", rig.scale.get());
        assert!((rig.zoom_next_frame() - 1.65).abs() < 1e-4);
    }

    #[test]
    fn the_grip_stops_at_the_users_scale_limits_on_a_scaled_display() {
        for host_scale in [1.5, 2.0] {
            for (to, limit) in [
                (pos2(100_000.0, 100_000.0), scale::MAX),
                (pos2(0.0, 0.0), scale::MIN),
            ] {
                let mut rig = settled(host_scale);

                rig.drag_grip(host_scale, to);

                let scale = rig.scale.get();
                assert!((scale - limit).abs() < 1e-4, "{scale}");
                assert!((rig.zoom_next_frame() / host_scale - limit).abs() < 1e-4);
            }
        }
    }

    /// Whether a second grip move goes through with the window `off` points from the canvas, after
    /// the first move's zoom has taken effect.
    fn second_move_goes_through(off: Vec2) -> bool {
        let mut rig = settled(1.0);
        rig.drag_grip(1.0, GRIP + vec2(72.0, 0.0));
        let first = rig.scale.get();
        let zoom = rig.zoom_next_frame();
        frame(
            &mut rig.canvas,
            &rig.ctx,
            10.04,
            (SIZE + off) * zoom,
            vec![Event::PointerMoved(GRIP + vec2(144.0, 0.0))],
        );
        (rig.scale.get() - first).abs() > 1e-4
    }

    #[test]
    fn the_grip_waits_until_the_host_has_resized_to_within_one_percent() {
        assert!(second_move_goes_through(Vec2::ZERO));
        assert!(second_move_goes_through(vec2(3.0, 0.0)));
        assert!(second_move_goes_through(vec2(0.0, 2.0)));
        assert!(second_move_goes_through(vec2(-3.0, 2.0)));
        assert!(!second_move_goes_through(vec2(20.0, 0.0)));
        assert!(!second_move_goes_through(vec2(0.0, 20.0)));
        assert!(!second_move_goes_through(vec2(-20.0, 0.0)));
    }

    #[test]
    fn while_waiting_for_the_host_the_grip_asks_for_a_frame_when_its_patience_ends() {
        let mut rig = settled(1.0);
        rig.drag_grip(1.0, GRIP + vec2(72.0, 0.0));
        // The zoom takes effect at 10.03 s, and the host doesn't grow the window.
        let events = vec![Event::PointerMoved(GRIP + vec2(144.0, 0.0))];
        frame(&mut rig.canvas, &rig.ctx, 10.03, SIZE, events);
        frame(&mut rig.canvas, &rig.ctx, 10.04, SIZE, vec![]);

        let waiting = frame(&mut rig.canvas, &rig.ctx, 10.05, SIZE, vec![]);

        assert!((rig.scale.get() - 1.1).abs() < 1e-4, "{}", rig.scale.get());
        let left = waiting.repaint.as_secs_f64();
        assert!((left - (GRIP_PATIENCE - 0.03)).abs() < 0.01, "{left}");
    }

    #[test]
    fn the_grip_is_drawn_inside_the_canvass_corner() {
        let mut rig = settled(1.0);

        let drawn = frame(&mut rig.canvas, &rig.ctx, 10.1, SIZE, vec![]);

        let corner = Rect::from_min_max(SIZE.to_pos2() - Vec2::splat(GRIP_SIZE), SIZE.to_pos2());
        assert_eq!(drawn.lines.len(), 3);
        for [start, end] in drawn.lines {
            assert!(
                corner.contains(start) && corner.contains(end),
                "{start:?} {end:?}"
            );
            // Each runs up and to the right.
            assert!(start.x < end.x && start.y > end.y, "{start:?} {end:?}");
        }
    }

    #[test]
    fn reopening_on_linux_keeps_the_scale_even_after_the_users_scale_changes() {
        reopening_with_native(&[1.645_833_4; 4], 1.0);
    }

    #[test]
    fn reopening_accounts_for_a_changed_native_scale() {
        reopening_with_native(&[1.0, 1.5, 1.5, 1.0], 1.5);
    }

    fn reopening_with_native(natives: &[f32], host: f32) {
        let scale = Arc::new(EditorScale::default());
        let mut canvas = Canvas::new(
            SIZE,
            Arc::clone(&scale),
            Colour {
                signal: egui::Color32::WHITE,
                lit: egui::Color32::WHITE,
            },
        );
        let mut restored = 1.0;
        for (&native, user) in natives.iter().zip([1.0, 1.25, 1.25, 0.75]) {
            scale.set(user);
            let ctx = egui::Context::default();
            ctx.set_zoom_factor(restored);
            canvas.opened();
            for (time, window) in [
                (0.0, Vec2::splat(10_000.0)),
                (0.01, SIZE * host),
                (0.02, SIZE * host),
            ] {
                frame_with_native(&mut canvas, &ctx, time, window, vec![], native);
            }
            restored = ctx.zoom_factor();
            assert!(
                (restored - host * user / native).abs() < 1e-5,
                "native {native}, user {user}: got zoom {restored}"
            );
            assert!((scale.get() - user).abs() < 1e-5);
        }
    }

    #[test]
    fn a_grip_move_waiting_for_the_host_is_applied_when_the_host_catches_up() {
        delayed_grip_move(false, false);
    }

    #[test]
    fn a_grip_move_is_kept_after_release_while_waiting_for_the_host() {
        delayed_grip_move(true, false);
    }

    #[test]
    fn a_grip_move_is_applied_after_patience_runs_out_without_another_event() {
        delayed_grip_move(false, true);
        delayed_grip_move(true, true);
    }

    fn delayed_grip_move(release: bool, host_refuses: bool) {
        let ctx = egui::Context::default();
        let scale = Arc::new(EditorScale::default());
        let colour = Colour {
            signal: egui::Color32::WHITE,
            lit: egui::Color32::WHITE,
        };
        let mut canvas = Canvas::new(SIZE, Arc::clone(&scale), colour);
        let start = pos2(712.0, 412.0);
        // Settle the opening resize before the grip receives its first press.
        for time in [-0.02, -0.01, 0.0] {
            frame(&mut canvas, &ctx, time, SIZE, vec![]);
        }
        frame(
            &mut canvas,
            &ctx,
            0.01,
            SIZE,
            vec![
                Event::PointerMoved(start),
                Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::default(),
                },
            ],
        );
        frame(
            &mut canvas,
            &ctx,
            0.02,
            SIZE,
            vec![Event::PointerMoved(start + vec2(72.0, 42.0))],
        );
        assert!(
            (scale.get() - 1.1).abs() < 1e-5,
            "first move: {}",
            scale.get()
        );
        // The zoom takes effect before the host grows the window. egui-baseview converted
        // this move using the previous frame's zoom (1).
        let latest = start + vec2(144.0, 84.0);
        let mut events = vec![Event::PointerMoved(latest)];
        if release {
            events.push(Event::PointerButton {
                pos: latest,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::default(),
            });
        }
        frame(&mut canvas, &ctx, 0.03, SIZE, events);
        assert!((scale.get() - 1.1).abs() < 1e-5, "wait for the host");
        // No further pointer event: either the host catches up or the patience expires.
        let time = if host_refuses { 0.23 } else { 0.04 };
        let window = if host_refuses { SIZE } else { SIZE * 1.1 };
        frame(&mut canvas, &ctx, time, window, vec![]);
        assert!(
            (scale.get() - 1.2).abs() < 1e-5,
            "latest move was lost: {}",
            scale.get()
        );
        // Apply the last resize and keep the pointer still across several zoomed frames.
        for step in 1..=3 {
            frame(
                &mut canvas,
                &ctx,
                f64::from(step).mul_add(0.01, time),
                SIZE * 1.2,
                vec![],
            );
            assert!((ctx.zoom_factor() - 1.2).abs() < 1e-5);
            assert!((scale.get() - 1.2).abs() < 1e-5);
        }
    }

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
