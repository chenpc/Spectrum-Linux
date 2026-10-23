#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

//! egui visuals and spacing for the Photos-like design.
//!
//! Values come from `tokens.rs`, which `cargo xtask theme` generates from
//! `assets/design/theme.json`; that file's colour roles are checked against the design document by
//! `cargo xtask design`. This module only maps the values onto egui.

use egui::{Color32, CornerRadius, FontFamily, FontId, Frame, Stroke, TextStyle, Visuals};

use crate::tokens::{color, radius, size, space, text, FONT_FAMILY};

/// The palette the chrome draws with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    pub window: Color32,
    pub titlebar: Color32,
    pub sidebar: Color32,
    pub detail: Color32,
    pub hairline: Color32,
    pub divider: Color32,
    pub text: Color32,
    pub ink: Color32,
    pub label: Color32,
    pub secondary: Color32,
    pub muted: Color32,
    pub row: Color32,
    pub row_selected_text: Color32,
    pub accent: Color32,
    pub track: Color32,
    pub control: Color32,
    pub selected: Color32,
    pub segmented: Color32,
    pub tile: Color32,
    pub on_accent: Color32,
    pub scrim: Color32,
}

impl Tokens {
    /// The light theme measured from the design document.
    pub fn photos() -> Self {
        Tokens {
            window: color::WINDOW,
            titlebar: color::TITLEBAR,
            sidebar: color::SIDEBAR,
            detail: color::DETAIL,
            hairline: color::HAIRLINE,
            divider: color::DIVIDER,
            text: color::TEXT,
            ink: color::INK,
            label: color::LABEL,
            secondary: color::SECONDARY,
            muted: color::MUTED,
            row: color::ROW,
            row_selected_text: color::ROW_SELECTED_TEXT,
            accent: color::ACCENT,
            track: color::TRACK,
            control: color::CONTROL,
            selected: color::SELECTED,
            segmented: color::SEGMENTED,
            tile: color::TILE,
            on_accent: color::ON_ACCENT,
            scrim: color::SCRIM,
        }
    }

    /// A filled frame with no border, for panels.
    pub fn panel(&self, fill: Color32) -> Frame {
        Frame::new()
            .fill(fill)
            .inner_margin(0.0)
            .stroke(Stroke::NONE)
    }
}

/// `Inter` is named in the design but not bundled here (it is OFL and not yet vendored), so egui's
/// embedded faces draw the interface. `FONT_FAMILY` records the intent and `install_font` uses a
/// file when one is given.
pub fn font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

/// Headings. egui has no bold family to select here, so emphasis comes from `RichText::strong()`
/// (which brightens the text colour); the design's Inter SemiBold needs `install_font`.
pub fn strong(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

/// A face for scripts Inter has no glyphs for. Folder names here are Chinese, and egui draws a
/// missing glyph as a tofu box, so the titlebar used to show `□□□□□` for a Chinese folder name. A ~20 MB CJK
/// face is not vendored into the repo; a system font is used instead, and `SPECTRUM_CJK_FONT`
/// overrides the search (e.g. `…/arphic/uming.ttc` for a Traditional-Chinese-shaped face).
pub const CJK_FAMILY: &str = "CJK fallback";
pub const CJK_CANDIDATES: [&str; 4] = [
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/arphic/uming.ttc",
    "/usr/share/fonts/truetype/arphic/ukai.ttc",
];

/// Inter first, then the CJK face for glyphs Inter lacks; monospace gets the same order.
pub fn font_definitions(inter: Option<Vec<u8>>, cjk: Option<Vec<u8>>) -> egui::FontDefinitions {
    let mut defs = egui::FontDefinitions::default();
    if let Some(bytes) = inter {
        defs.font_data.insert(
            FONT_FAMILY.to_string(),
            std::sync::Arc::new(egui::FontData::from_owned(bytes)),
        );
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            if let Some(list) = defs.families.get_mut(&family) {
                list.insert(0, FONT_FAMILY.to_string());
            }
        }
    }
    if let Some(bytes) = cjk {
        defs.font_data.insert(
            CJK_FAMILY.to_string(),
            std::sync::Arc::new(egui::FontData::from_owned(bytes)),
        );
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            if let Some(list) = defs.families.get_mut(&family) {
                list.push(CJK_FAMILY.to_string());
            }
        }
    }
    defs
}

/// Embed an Inter file (path to a `.ttf`/`.otf`) as the interface face. Without it the app still
/// runs; `install_font` reports whether it loaded.
pub fn install_font(ctx: &egui::Context, path: &std::path::Path) -> bool {
    match std::fs::read(path) {
        Ok(bytes) => {
            ctx.set_fonts(font_definitions(Some(bytes), None));
            true
        }
        Err(_) => false,
    }
}

/// Install Inter (if the file is there) plus the first readable CJK fallback from `candidates`.
/// Returns the fallback's path, so the launch log can say what actually loaded.
pub fn install_fonts(
    ctx: &egui::Context,
    inter: &std::path::Path,
    candidates: &[&str],
) -> Option<String> {
    let loaded = candidates
        .iter()
        .map(std::path::PathBuf::from)
        .find(|path| path.is_file());
    let cjk = loaded.as_ref().and_then(|path| std::fs::read(path).ok());
    let inter_bytes = std::fs::read(inter).ok();
    ctx.set_fonts(font_definitions(inter_bytes, cjk));
    loaded.map(|path| path.display().to_string())
}

/// The design's `Visuals`: colours, corner radii, widget strokes. Split out from `apply` so a test
/// can assert the palette without a context.
pub fn visuals(t: &Tokens) -> Visuals {
    let mut v = Visuals::light();
    v.panel_fill = t.window;
    v.window_fill = t.window;
    v.extreme_bg_color = t.window;
    v.faint_bg_color = t.selected;
    v.override_text_color = Some(t.text);
    v.hyperlink_color = t.accent;
    v.window_stroke = Stroke::new(1.0, t.divider);
    v.window_corner_radius = CornerRadius::same(radius::WINDOW as u8);
    v.menu_corner_radius = CornerRadius::same(radius::CONTROL as u8);
    v.selection.bg_fill = t.accent;
    v.selection.stroke = Stroke::new(1.0, t.on_accent);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.hairline);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, t.text);
    v.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    v.widgets.inactive.bg_fill = t.control;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, t.ink);
    v.widgets.inactive.bg_stroke = Stroke::NONE;
    v.widgets.hovered.weak_bg_fill = t.selected;
    v.widgets.hovered.bg_fill = t.selected;
    v.widgets.hovered.bg_stroke = Stroke::NONE;
    v.widgets.active.weak_bg_fill = t.selected;
    v.widgets.active.bg_fill = t.selected;
    v.widgets.active.bg_stroke = Stroke::NONE;
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
    ] {
        w.corner_radius = CornerRadius::same(radius::CONTROL as u8);
    }
    v
}

/// Apply the design to the context: colours, corner radii, spacing, text sizes.
pub fn apply(ctx: &egui::Context) {
    let t = Tokens::photos();
    ctx.set_visuals(visuals(&t));

    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 4.0);
        s.spacing.button_padding = egui::vec2(space::CONTROL_INLINE, 3.0);
        s.spacing.interact_size.y = size::CONTROL_HEIGHT;
        // Overlay scrollbars, like macOS: they float over the content and allocate no width, so the
        // grid keeps its full 1168 px (seven 166 px columns) instead of losing a column to a
        // permanent bar at the panel's right edge.
        let scroll = &mut s.spacing.scroll;
        scroll.floating = true;
        scroll.bar_width = 8.0;
        scroll.floating_width = 4.0;
        scroll.floating_allocated_width = 0.0;
        scroll.dormant_background_opacity = 0.0;
        scroll.dormant_handle_opacity = 0.0;
        scroll.foreground_color = true;
        s.text_styles.insert(TextStyle::Body, font(text::ROW_SIZE));
        s.text_styles
            .insert(TextStyle::Button, font(text::CONTROL_SIZE));
        s.text_styles
            .insert(TextStyle::Small, font(text::META_SIZE));
        s.text_styles
            .insert(TextStyle::Heading, strong(text::TITLE_SIZE));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_leaves_egui_showing_the_design_colors() {
        let ctx = egui::Context::default();
        apply(&ctx);
        let t = Tokens::photos();
        let style = ctx.global_style();
        assert_eq!(style.spacing.interact_size.y, 26.0, "control height");
        assert_eq!(style.text_styles[&TextStyle::Body].size, 13.0, "row text");
        assert_eq!(style.text_styles[&TextStyle::Small].size, 11.0, "meta text");

        let v = visuals(&t);
        assert_eq!(v.panel_fill, t.window);
        assert_eq!(v.selection.bg_fill, t.accent);
        assert_eq!(v.widgets.hovered.bg_fill, t.selected);
        assert_eq!(v.widgets.inactive.bg_fill, t.control);
    }

    #[test]
    fn a_missing_font_file_is_reported_not_panic() {
        let ctx = egui::Context::default();
        assert!(!install_font(
            &ctx,
            std::path::Path::new("/nonexistent/Inter.ttf")
        ));
    }
}
