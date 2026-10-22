#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

//! The design system's widgets: the pieces the screens are built from, drawn with `theme::Tokens`.
//!
//! Every widget is an egui `Button`/`Label` under the hood, so AccessKit exposes it and
//! `egui_kittest` can drive tests by label — the tests never reach into app internals.

use egui::{
    Color32, Context, CornerRadius, FontFamily, FontId, Frame, Image, Rect, Response, RichText, Ui,
    Vec2,
};

use crate::theme::Tokens;
use crate::tokens::{radius, size, space, text};

/// The widget registry: every interactive widget reports a stable id and its rect once per frame.
///
/// Tests and the control channel address widgets by id (`ui.click {id: "segment:view:All"}`), never
/// by coordinates or by matching painted text — a control that moves keeps working, and a control
/// that disappeared fails loudly instead of clicking whatever happens to be at that point.
#[derive(Clone, Debug, PartialEq)]
pub struct Widget {
    pub id: String,
    pub rect: Rect,
    /// `button` | `segment` | `cell` | `row` | `tile` | `badge` | `field` | `panel` | `scroll` | `slider`.
    pub kind: &'static str,
}

#[derive(Clone, Default)]
struct Registry(Vec<Widget>);

/// Drop last frame's entries: a control that is gone this frame must not stay clickable.
pub fn begin_frame(ctx: &Context) {
    ctx.data_mut(|d| {
        d.insert_temp(
            egui::Id::new("spectrum-widget-registry"),
            Registry::default(),
        )
    });
}

/// Record one widget. Called from every widget helper, and by the screen for widgets it draws
/// itself (panels, scroll areas, text fields, clickable headings).
pub fn register(ui: &Ui, id: &str, rect: Rect, kind: &'static str) {
    if rect.is_negative() {
        return;
    }
    ui.ctx().data_mut(|d| {
        let entry = d.get_temp::<Registry>(egui::Id::new("spectrum-widget-registry"));
        let mut list = entry.map(|r| r.0).unwrap_or_default();
        list.push(Widget {
            id: id.to_string(),
            rect,
            kind,
        });
        d.insert_temp(egui::Id::new("spectrum-widget-registry"), Registry(list));
    });
}

/// Everything registered in the frame that just ran.
pub fn registry(ctx: &Context) -> Vec<Widget> {
    ctx.data(|d| {
        d.get_temp::<Registry>(egui::Id::new("spectrum-widget-registry"))
            .map(|r| r.0)
            .unwrap_or_default()
    })
}

/// One widget by exact id, or by substring when `filter` is a substring instead of a full id.
pub fn find(ctx: &Context, id: &str) -> Option<Widget> {
    let all = registry(ctx);
    all.iter()
        .find(|w| w.id == id)
        .cloned()
        .or_else(|| all.iter().find(|w| w.id.contains(id)).cloned())
}

/// A 1px rule in the hairline colour, spanning the available width.
pub fn hairline(ui: &mut Ui, t: &Tokens) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 1.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::ZERO, t.hairline);
}

/// Section label: 11 px semibold in the secondary colour (`Albums`, `Filters`).
pub fn section_header(ui: &mut Ui, t: &Tokens, title: &str) {
    ui.add_space(4.0);
    ui.label(
        RichText::new(title)
            .size(text::SECTION_SIZE)
            .color(t.secondary)
            .strong(),
    );
}

/// A sidebar row: 26 px tall, 5 px radius, 8 px inline padding.
pub fn sidebar_row(ui: &mut Ui, t: &Tokens, id: &str, label: &str, selected: bool) -> Response {
    let fill = if selected {
        t.selected
    } else {
        Color32::TRANSPARENT
    };
    let fg = if selected { t.row_selected_text } else { t.row };
    let width = ui.available_width();
    let response = ui.add_sized(
        [width, size::ROW_HEIGHT],
        egui::Button::new(RichText::new(label).size(text::ROW_SIZE).color(fg))
            .min_size(Vec2::new(width, size::ROW_HEIGHT))
            .fill(fill)
            .corner_radius(radius::ROW)
            .stroke(egui::Stroke::NONE),
    );
    register(ui, id, response.rect, "row");
    response
}

/// A segmented control (`All Photos / Months / Years`, `Adjust / Info`): a tinted track with
/// pill-shaped cells sized to their labels, the selected one on white. Returns the index the user
/// picked this frame.
pub fn segmented(
    ui: &mut Ui,
    t: &Tokens,
    id: &str,
    labels: &[&str],
    selected: Option<usize>,
) -> Option<usize> {
    let outer_response = Frame::new()
        .fill(t.segmented)
        .corner_radius(radius::CONTROL)
        .inner_margin(2.0)
        .stroke(egui::Stroke::NONE)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(2.0, 0.0);
                let mut picked = None;
                for (i, label) in labels.iter().enumerate() {
                    let is_on = selected == Some(i);
                    let fg = if is_on { t.text } else { t.muted };
                    let fill = if is_on {
                        t.window
                    } else {
                        Color32::TRANSPARENT
                    };
                    let response = ui.add_sized(
                        // Width hugs the label: sizing cells from `available_width` would stretch
                        // the track and push everything after it past the window edge.
                        [0.0, size::SEGMENT_HEIGHT],
                        egui::Button::new(RichText::new(*label).size(text::SEGMENT_SIZE).color(fg))
                            .min_size(Vec2::new(0.0, size::SEGMENT_HEIGHT))
                            .fill(fill)
                            .corner_radius(radius::PILL)
                            .stroke(egui::Stroke::NONE),
                    );
                    register(ui, &format!("{id}:{label}"), response.rect, "cell");
                    if response.clicked() {
                        picked = Some(i);
                    }
                }
                picked
            })
            .inner
        });
    register(ui, id, outer_response.response.rect, "segment");
    outer_response.inner
}

/// A chrome button: `control` (grey) or `primary` (accent fill, white text).
pub fn control_button(ui: &mut Ui, t: &Tokens, id: &str, label: &str, primary: bool) -> Response {
    let fill = if primary { t.accent } else { t.control };
    let fg = if primary { t.on_accent } else { t.ink };
    let response = ui.add(
        egui::Button::new(RichText::new(label).size(text::CONTROL_SIZE).color(fg))
            .min_size(Vec2::new(0.0, size::CONTROL_HEIGHT))
            .fill(fill)
            .corner_radius(radius::CONTROL)
            .stroke(egui::Stroke::NONE),
    );
    register(ui, id, response.rect, "button");
    response
}

/// The HDR badge: a small pill in the accent colour, or neutral for SDR.
pub fn badge(ui: &mut Ui, t: &Tokens, id: &str, label: &str, on: bool) {
    let frame = Frame::new()
        .fill(if on { t.accent } else { t.control })
        .corner_radius(radius::PILL)
        .inner_margin(egui::Margin::symmetric(6, 2))
        .stroke(egui::Stroke::NONE);
    let shown = frame.show(ui, |ui| {
        let fg = if on { t.on_accent } else { t.ink };
        ui.label(RichText::new(label).size(text::CHIP_SIZE).color(fg));
    });
    register(ui, id, shown.response.rect, "badge");
}

/// What a grid cell draws. `image` is a decoded thumbnail cropped to the square cell; without it
/// the cell is the design's placeholder colour with the file name visible.
pub struct TileSpec<'a> {
    pub label: &'a str,
    pub badge: &'a str,
    pub badge_on: bool,
    pub selected: bool,
    pub image: Option<&'a egui::TextureHandle>,
}

/// A grid cell: a `164×164` tile — the thumbnail, or the placeholder colour with the name — and the
/// HDR/SDR badge inside it at the top-left. Inside, because the design's grid rows sit on a 166 px
/// pitch: a badge under the tile would make every row ~185 px tall. The badge's registry id is the
/// tile's with `tile:` swapped for `badge:`.
pub fn tile(ui: &mut Ui, t: &Tokens, id: &str, spec: TileSpec<'_>) -> Response {
    let side = size::TILE_SIZE;
    let fill = if spec.selected { t.selected } else { t.tile };
    let font = FontId::new(text::CHIP_SIZE, FontFamily::Proportional);
    let pill_w = ui.fonts_mut(|f| {
        let mut width = 0.0;
        for c in spec.badge.chars() {
            width += f.glyph_width(&font, c);
        }
        width
    }) + 12.0;

    let cell = ui.allocate_ui_with_layout(
        Vec2::new(side, side),
        egui::Layout::top_down(egui::Align::LEFT),
        |ui| {
            let rect = ui.max_rect();
            ui.painter().rect_filled(rect, radius::TILE, fill);

            let response = match spec.image {
                // The image is the tile: square centre-crop, rounded like the design's cells, and
                // named by `alt_text` so AccessKit (and `get_by_label`) still finds it.
                Some(texture) => {
                    // The pixels are already square (`crop_center_square`), so exact sizing neither
                    // stretches nor crops.
                    ui.put(
                        rect,
                        Image::from_texture(texture)
                            .fit_to_exact_size(Vec2::new(side, side))
                            .corner_radius(radius::TILE)
                            .alt_text(spec.label)
                            .sense(egui::Sense::click()),
                    )
                }
                // No thumbnail yet: the name is both the click target and the accessible label.
                None => ui.put(
                    rect,
                    egui::Label::new(
                        RichText::new(spec.label)
                            .size(text::CHIP_SIZE)
                            .color(t.label),
                    )
                    .sense(egui::Sense::click()),
                ),
            };

            let pill = Rect::from_min_size(rect.min + Vec2::new(6.0, 6.0), Vec2::new(pill_w, 18.0));
            ui.painter().rect_filled(
                pill,
                radius::PILL,
                if spec.badge_on { t.accent } else { t.control },
            );
            let shown = ui.put(
                pill,
                egui::Label::new(
                    RichText::new(spec.badge)
                        .size(text::CHIP_SIZE)
                        .color(if spec.badge_on { t.on_accent } else { t.ink }),
                )
                .sense(egui::Sense::hover()),
            );
            register(
                ui,
                &format!("badge:{}", id.trim_start_matches("tile:")),
                shown.rect.union(pill),
                "badge",
            );
            if spec.selected {
                ui.painter().rect_stroke(
                    rect,
                    radius::TILE,
                    egui::Stroke::new(1.5, t.accent),
                    egui::StrokeKind::Inside,
                );
            }
            response
        },
    );
    let response = cell.inner;
    register(ui, id, response.rect.union(cell.response.rect), "tile");
    response
}

/// A labelled slider row from the inspector: name on the left, value on the right, a 2 px track
/// with the filled part in the accent colour. `fraction` is 0..1.
pub fn slider_row(
    ui: &mut Ui,
    t: &Tokens,
    id: &str,
    name: &str,
    value: &str,
    fraction: f32,
) -> f32 {
    let width = ui.available_width();
    ui.label(
        RichText::new(name)
            .size(text::CONTROL_SIZE + 1.0)
            .color(t.label),
    );
    ui.label(
        RichText::new(value)
            .size(text::META_SIZE)
            .color(t.secondary),
    );
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 12.0), egui::Sense::hover());
    let mid = rect.center().y;
    let filled = rect.min.x + width * fraction.clamp(0.0, 1.0);
    ui.painter().rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(rect.min.x, mid - 1.0),
            egui::pos2(filled, mid + 1.0),
        ),
        CornerRadius::same(1),
        t.accent,
    );
    ui.painter().rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(filled, mid - 1.0),
            egui::pos2(rect.max.x, mid + 1.0),
        ),
        CornerRadius::same(1),
        t.track,
    );
    ui.painter().circle(
        egui::pos2(filled, mid),
        radius::KNOB,
        t.window,
        egui::Stroke::new(1.0, t.divider),
    );
    register(ui, id, rect, "slider");
    width
}

/// Centred caption: a bold first line and a secondary metadata line.
pub fn caption(ui: &mut Ui, t: &Tokens, title: &str, meta: &str) {
    Frame::new().inner_margin(0.0).show(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(0.0, 3.0);
            ui.label(
                RichText::new(title)
                    .size(text::CAPTION_SIZE)
                    .color(t.caption())
                    .strong(),
            );
            ui.label(RichText::new(meta).size(text::META_SIZE).color(t.secondary));
        });
    });
}

impl Tokens {
    fn caption(&self) -> Color32 {
        self.label
    }
}

/// A round 34 px nav button on a translucent scrim (detail view's previous/next).
pub fn nav_button(ui: &mut Ui, t: &Tokens, id: &str, label: &str, side: f32) -> Response {
    let response = ui.add(
        egui::Button::new(RichText::new(label).size(text::CONTROL_SIZE).color(t.text))
            .min_size(Vec2::new(side, side))
            .fill(t.scrim)
            .corner_radius(radius::CIRCLE)
            .stroke(egui::Stroke::NONE),
    );
    register(ui, id, response.rect, "button");
    response
}

/// Vertical padding used between inspector groups.
pub fn group_gap(ui: &mut Ui) {
    ui.add_space(space::SECTION_GAP);
}

/// Align helper for the detail view's centred stage: content is centred in the available width.
pub fn centered(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.horizontal_centered(add);
}
