//! The "Add Folder" browser: a floating window that offers directories, not paths to type.
//!
//! The library only takes directories, so the browser never shows files, and picking one goes
//! through `folder.add` like every other mutation — including duplicates and directories that
//! vanished between listing and picking, which the command reports and the window leaves open so
//! the reason is visible. Keyboard confirmation lives in `app.rs`, where it can be sequenced against
//! the path field and the detail view instead of firing for both.

use std::path::PathBuf;

use egui::{Context, Id, ScrollArea, Ui, Window};

use crate::theme::Tokens;
use crate::tokens::{space, text};
use crate::widgets;

/// What the user asked for this frame. The caller owns the library, so the browser only reports.
pub enum Action {
    Nothing,
    /// Add the directory the window is showing.
    Open,
    Close,
}

/// Which directory the browser is showing.
pub struct Browser {
    path: PathBuf,
    /// Why the current directory could not be read, if it could not.
    error: Option<String>,
}

impl Browser {
    pub fn new(path: PathBuf) -> Self {
        let mut browser = Self { path, error: None };
        browser.refresh();
        browser
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Start at the directory above the library's open folder, or in the home directory when the
    /// library has none — that is where someone browsing for a new folder expects to land.
    pub fn start_at(open_folder: &std::path::Path) -> Self {
        let start = if open_folder.as_os_str().is_empty() {
            spectrum_core::folder_browser::home()
        } else {
            spectrum_core::folder_browser::parent(open_folder)
                .unwrap_or_else(|| open_folder.to_path_buf())
        };
        Self::new(start)
    }

    fn refresh(&mut self) {
        // The reason, when there is one: an empty list would read like an empty folder.
        self.error = spectrum_core::folder_browser::listing(&self.path).err();
    }

    fn enter(&mut self, path: PathBuf) {
        self.path = path;
        self.refresh();
    }

    fn up(&mut self) {
        if let Some(parent) = spectrum_core::folder_browser::parent(&self.path) {
            self.enter(parent);
        }
    }
}

/// Draw the floating window and report what was clicked.
pub fn draw(ctx: &Context, tokens: &Tokens, browser: &mut Browser) -> Action {
    let mut action = Action::Nothing;
    Window::new("Add Folder")
        .id(Id::new("spectrum-folder-browser"))
        .default_pos([520.0, 170.0])
        .default_size([360.0, 430.0])
        .collapsible(false)
        .show(ctx, |ui| {
            ui.set_width(340.0);
            draw_body(ui, tokens, browser, &mut action);
            widgets::register(ui, "panel:browser", ui.min_rect(), "panel");
        });
    action
}

fn draw_body(ui: &mut Ui, tokens: &Tokens, browser: &mut Browser, action: &mut Action) {
    // Register the label's own rect: `ui.cursor()` is where the *next* widget starts, whose height
    // is unbounded, and the control channel would click a rect that reaches infinity.
    let path = ui.label(
        egui::RichText::new(browser.path().display().to_string())
            .size(text::META_SIZE)
            .color(tokens.secondary),
    );
    widgets::register(ui, "info:browser-path", path.rect, "row");
    ui.add_space(space::CONTROL_INLINE);

    if let Some(error) = browser.error.as_ref() {
        let shown = ui.label(
            egui::RichText::new(error)
                .size(text::CAPTION_SIZE)
                .color(tokens.accent),
        );
        widgets::register(ui, "info:browser-error", shown.rect, "row");
    }

    let entries = spectrum_core::folder_browser::listing(browser.path()).unwrap_or_default();
    let mut picked: Option<PathBuf> = None;
    let scroll = ScrollArea::vertical()
        .auto_shrink([false, true])
        .max_height(300.0)
        .show(ui, |ui| {
            if entries.is_empty() && browser.error.is_none() {
                ui.label(
                    egui::RichText::new("No folders inside")
                        .size(text::ROW_SIZE)
                        .color(tokens.muted),
                );
            }
            for entry in &entries {
                // The count of media files directly inside is what makes the right folder findable
                // without opening three of them.
                let label = match entry.media {
                    0 => entry.name.clone(),
                    media => format!("{}  ·  {media}", entry.name),
                };
                let row =
                    widgets::sidebar_row(ui, tokens, &format!("dir:{}", entry.name), &label, false);
                if row.clicked() {
                    picked = Some(entry.path.clone());
                }
            }
        });
    widgets::register(ui, "browser:scroll", scroll.inner_rect, "scroll");
    ui.add_space(space::CONTROL_INLINE);

    ui.horizontal(|ui| {
        let up = widgets::control_button(
            ui,
            tokens,
            "button:browser-up",
            "Up",
            spectrum_core::folder_browser::parent(browser.path()).is_some(),
        );
        if up.clicked() {
            browser.up();
        }
        ui.add_space(space::CONTROL_INLINE);
        if widgets::control_button(ui, tokens, "button:browser-cancel", "Cancel", false).clicked() {
            *action = Action::Close;
        }
        ui.add_space(space::CONTROL_INLINE);
        // The design's prompt is "Scan": adding a folder means scanning it, and the macOS reference
        // labels its panel button the same way.
        if widgets::control_button(ui, tokens, "button:browser-scan", "Scan", true).clicked() {
            *action = Action::Open;
        }
    });

    if let Some(path) = picked {
        browser.enter(path);
    }
}
