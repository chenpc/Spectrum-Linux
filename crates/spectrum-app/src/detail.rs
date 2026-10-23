//! The single-image view: the selected file at its own size, with zoom and pan.
//!
//! The FigCraft prototype has no detail screen, so this is the design's *language* applied to a
//! screen the prototype never covered: the canvas fills the main column, the controls use the same
//! 26 px / 6 px-radius control style as the toolbar, and the inspector stays on the right.
//!
//! Zoom is stored as displayed pixels per source pixel, so `1.0` is exactly 1:1, the readout is that
//! number times 100, and `Fit` is whatever scale keeps the whole image inside the canvas. The zoom
//! value lives in the library model (so it goes through the command registry); only the pan is UI
//! state, because nothing but the canvas cares where the picture is centred.

use egui::{Image, Rect, Sense, Ui, Vec2};

use crate::theme;
use crate::theme::Tokens;
use crate::tokens::{size, text};
use crate::widgets;

/// The strip above the canvas: control height plus 6 px above and below, like the toolbar.
const STRIP_HEIGHT: f32 = size::CONTROL_HEIGHT + 12.0;

/// Zoom bounds. Below the lower bound the image is a smudge; above the upper bound a single pixel
/// fills the screen.
const MIN_SCALE: f32 = 0.02;
const MAX_SCALE: f32 = 16.0;

/// Per-file canvas state. Deliberately tiny: a different file must not inherit another file's pan.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasState {
    pub pan: Vec2,
    /// The scale the frame drew at, and where. Recomputed every frame from the canvas size.
    pub scale: f32,
    pub image_rect: Rect,
}

impl Default for CanvasState {
    fn default() -> Self {
        CanvasState {
            pan: Vec2::ZERO,
            scale: 1.0,
            image_rect: Rect::NOTHING,
        }
    }
}

/// Fit-to-canvas scale: the largest scale that keeps the whole image inside the canvas.
pub fn fit_scale(image: Vec2, canvas: Vec2) -> f32 {
    (canvas.x / image.x.max(1.0))
        .min(canvas.y / image.y.max(1.0))
        .max(0.01)
}

/// The model's zoom (`None` = fit) resolved against the canvas.
pub fn resolve_scale(zoom: Option<f32>, image: Vec2, canvas: Vec2) -> f32 {
    match zoom {
        Some(scale) => scale.clamp(MIN_SCALE, MAX_SCALE),
        None => fit_scale(image, canvas),
    }
}

/// Where the image goes for a scale and pan.
pub fn image_rect_for(canvas: Rect, image: Vec2, scale: f32, pan: Vec2) -> Rect {
    Rect::from_center_size(canvas.center() + pan, image * scale)
}

/// How far the picture may be dragged. The limit is the overflow past the canvas, halved: an image
/// smaller than the canvas cannot be moved at all (it stays centred), and a larger one stops the
/// moment an edge reaches the canvas edge, so no strip of background appears.
pub fn clamp_pan(canvas: Rect, image: Vec2, scale: f32, pan: Vec2) -> Vec2 {
    let drawn = image * scale;
    let limit = Vec2::new(
        ((drawn.x - canvas.width()) * 0.5).max(0.0),
        ((drawn.y - canvas.height()) * 0.5).max(0.0),
    );
    Vec2::new(
        pan.x.clamp(-limit.x, limit.x),
        pan.y.clamp(-limit.y, limit.y),
    )
}

/// The zoom bounds the `detail.zoom` command clamps to, kept in one place with the canvas.
pub fn scale_bounds() -> (f32, f32) {
    (MIN_SCALE, MAX_SCALE)
}

/// What the canvas did this frame.
pub struct DetailOutcome {
    /// `false` when the frame has not decoded yet (or cannot decode): the canvas then shows a
    /// placeholder colour and a message instead of an empty rectangle.
    pub painted: bool,
    /// The percentage shown in the strip.
    pub percent: u32,
    /// The canvas rect, registered as `canvas:detail`.
    pub rect: Rect,
    /// The Close button was pressed; the caller dispatches `detail.close`.
    pub close_clicked: bool,
    /// `detail.zoom` actions requested by the strip buttons, in press order.
    pub zoom_actions: Vec<&'static str>,
}

/// Draw the control strip and the canvas. `texture` is the decoded full-size frame; `None` means it
/// has not arrived, or the file cannot be decoded at all.
pub fn draw(
    ui: &mut Ui,
    t: &Tokens,
    texture: Option<&egui::TextureHandle>,
    zoom: Option<f32>,
    state: &mut CanvasState,
) -> DetailOutcome {
    let canvas_width = ui.available_width();
    let canvas_height = (ui.available_height() - STRIP_HEIGHT).max(64.0);
    // Computed before the strip is drawn, so the readout shows this frame's scale.
    let canvas = Rect::from_min_size(
        ui.cursor().min + Vec2::new(0.0, STRIP_HEIGHT),
        Vec2::new(canvas_width, canvas_height),
    );
    let image = texture.map(|tex| tex.size_vec2());
    let scale = image
        .map(|size| resolve_scale(zoom, size, canvas.size()))
        .unwrap_or(1.0);
    state.scale = scale;

    let mut zoom_actions: Vec<&'static str> = Vec::new();
    let mut close_clicked = false;
    let strip = ui.allocate_ui_with_layout(
        Vec2::new(canvas_width, STRIP_HEIGHT),
        egui::Layout::top_down(egui::Align::LEFT),
        |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if widgets::control_button(ui, t, "button:detail-zoom-out", "−", false).clicked()
                {
                    zoom_actions.push("out");
                }
                if widgets::control_button(ui, t, "button:detail-1x", "1:1", false).clicked() {
                    zoom_actions.push("one");
                }
                if widgets::control_button(ui, t, "button:detail-fit", "Fit", false).clicked() {
                    zoom_actions.push("fit");
                }
                if widgets::control_button(ui, t, "button:detail-zoom-in", "+", false).clicked() {
                    zoom_actions.push("in");
                }
                ui.label(
                    egui::RichText::new(format!("{}%", (scale * 100.0).round() as u32))
                        .size(text::META_SIZE)
                        .color(t.secondary),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::control_button(ui, t, "button:detail-close", "Close", false)
                        .clicked()
                    {
                        close_clicked = true;
                    }
                });
            });
        },
    );
    widgets::register(ui, "panel:detail-strip", strip.response.rect, "panel");
    widgets::hairline(ui, t);

    let painter = ui.painter_at(canvas);
    painter.rect_filled(canvas, 0.0, t.detail);

    let mut painted = false;
    if let (Some(texture), Some(size)) = (texture, image) {
        let rect = image_rect_for(canvas, size, scale, state.pan);
        // The picture senses nothing, and is drawn before the canvas rectangle is allocated: egui
        // gives the pointer to the *last* widget whose rect contains it, so allocating the canvas
        // afterwards is what makes dragging over the picture pan the picture.
        ui.put(
            rect,
            Image::from_texture(texture)
                .fit_to_exact_size(rect.size())
                .alt_text("photo")
                .sense(Sense::empty()),
        );
        state.image_rect = rect;
        painted = true;
    } else {
        state.image_rect = Rect::NOTHING;
        painter.text(
            canvas.center(),
            egui::Align2::CENTER_CENTER,
            "No preview available",
            theme::font(text::ROW_SIZE),
            t.secondary,
        );
    }

    // Allocated last, so the canvas wins the pointer over the image drawn on top of it.
    let response = ui.allocate_rect(canvas, Sense::drag());

    // Drag pans; the wheel pans unless Ctrl/⌘ is held, which zooms instead.
    if response.dragged() {
        state.pan = clamp_pan(
            canvas,
            image.unwrap_or(Vec2::ZERO),
            scale,
            state.pan + response.drag_delta(),
        );
    }
    let wheel = ui.input(|i| i.smooth_scroll_delta);
    if wheel != Vec2::ZERO {
        let zooming = ui.input(|i| i.modifiers.ctrl || i.modifiers.command);
        if zooming {
            zoom_actions.push(if wheel.y > 0.0 { "in" } else { "out" });
        } else {
            state.pan = clamp_pan(
                canvas,
                image.unwrap_or(Vec2::ZERO),
                scale,
                state.pan + wheel,
            );
        }
    }

    widgets::register(ui, "canvas:detail", canvas, "canvas");

    DetailOutcome {
        painted,
        percent: (scale * 100.0).round() as u32,
        rect: canvas,
        close_clicked,
        zoom_actions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(w: f32, h: f32) -> Rect {
        Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(w, h))
    }

    #[test]
    fn fit_is_the_largest_scale_that_fits() {
        let scale = fit_scale(Vec2::new(1600.0, 1200.0), Vec2::new(800.0, 600.0));
        assert_eq!(scale, 0.5, "both axes fit exactly");
        // Tall and narrow: the height decides, not the width.
        let scale = fit_scale(Vec2::new(100.0, 1000.0), Vec2::new(800.0, 400.0));
        assert_eq!(scale, 0.4);
        let rect = image_rect_for(
            rect(800.0, 600.0),
            Vec2::new(1600.0, 1200.0),
            scale,
            Vec2::ZERO,
        );
        assert!(rect.width() <= 800.0 && rect.height() <= 600.0);
    }

    #[test]
    fn explicit_zoom_is_used_and_clamped() {
        let canvas = Vec2::new(800.0, 600.0);
        let image = Vec2::new(1600.0, 1200.0);
        assert_eq!(resolve_scale(Some(1.0), image, canvas), 1.0, "1:1 is 1:1");
        assert_eq!(
            resolve_scale(Some(99.0), image, canvas),
            MAX_SCALE,
            "an absurd zoom is clamped, not honoured"
        );
        assert_eq!(resolve_scale(Some(0.0), image, canvas), MIN_SCALE);
        assert_eq!(
            resolve_scale(None, image, canvas),
            fit_scale(image, canvas),
            "None means fit"
        );
    }

    #[test]
    fn a_picture_smaller_than_the_canvas_cannot_be_panned() {
        let canvas = rect(800.0, 600.0);
        let pan = clamp_pan(
            canvas,
            Vec2::new(400.0, 300.0),
            1.0,
            Vec2::new(120.0, -80.0),
        );
        assert_eq!(pan, Vec2::ZERO, "fitting leaves nothing to pan");
    }

    #[test]
    fn panning_stops_where_the_picture_edge_meets_the_canvas() {
        let canvas = rect(800.0, 600.0);
        let image = Vec2::new(1600.0, 1200.0);
        let pan = clamp_pan(canvas, image, 1.0, Vec2::new(-900.0, 900.0));
        assert_eq!(pan, Vec2::new(-400.0, 300.0), "half the overflow each way");
        let drawn = image_rect_for(canvas, image, 1.0, pan);
        assert_eq!(
            drawn.max.x, 800.0,
            "one edge reached the canvas edge and stopped there"
        );
        assert!(
            drawn.min.x <= canvas.min.x
                && drawn.max.x >= canvas.max.x
                && drawn.min.x < canvas.min.x,
            "the canvas stays covered by the picture, never by background: {drawn:?} in {canvas:?}"
        );
    }

    #[test]
    fn bounds_match_the_command() {
        let (min, max) = scale_bounds();
        assert_eq!(min, spectrum_core::model::DETAIL_MIN_SCALE);
        assert_eq!(max, spectrum_core::model::DETAIL_MAX_SCALE);
    }
}
