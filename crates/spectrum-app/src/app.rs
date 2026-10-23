#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

//! The Library screen: titlebar, sidebar, tile grid, inspector, status bar.
//!
//! Every control dispatches a `spectrum-core::command` id and the UI never mutates the model
//! directly, so a test driven by labels exercises exactly the path a click does, and a rejected
//! command leaves the screen unchanged with its reason in the status bar.
//!
//! Geometry and colour come from `tokens.rs` (generated from the design document) through
//! `theme.rs` and `widgets.rs`. Tiles are solid cells with the file name: image decoding is not
//! wired yet, so nothing here pretends to show a photo.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use egui::{
    ColorImage, Context, Event, Frame, Layout, Margin, Panel, RichText, ScrollArea, TextureHandle,
    TextureOptions, Ui, Vec2,
};
use serde_json::{json, Value};

use spectrum_core::hdr::DisplayMode;
use spectrum_core::scan::{MediaEntry, MediaKind};
use spectrum_core::sort::{SortDirection, SortKey};
use spectrum_core::timeline::civil_from_days;
use spectrum_core::{dispatch, InspectorTab, Library, ViewMode};

use crate::control::{self, Inbox};
use crate::detail::{self, CanvasState};
use crate::theme::{self, Tokens};
use crate::thumbs::Worker;
use crate::tokens::{radius, size, space, text};
use crate::widgets::{self, TileSpec, Widget};

const VIEWS: [ViewMode; 3] = [ViewMode::All, ViewMode::Months, ViewMode::Years];
const TABS: [InspectorTab; 2] = [InspectorTab::Adjust, InspectorTab::Info];
const SORTS: [SortKey; 5] = [
    SortKey::Name,
    SortKey::DateModified,
    SortKey::Kind,
    SortKey::Transfer,
    SortKey::Size,
];

/// Was this key pressed with the platform's main modifier? `count_and_consume_key` matches
/// `Modifiers::COMMAND` only when `command` is set, and `Modifiers::CTRL` only when `ctrl` is, so
/// both patterns are tried: a Linux Ctrl arrives as `ctrl`, a macOS ⌘ as `command`.
fn zoom_key(ui: &Ui, key: egui::Key) -> bool {
    ui.input_mut(|i| {
        i.count_and_consume_key(egui::Modifiers::CTRL, key)
            + i.count_and_consume_key(egui::Modifiers::COMMAND, key)
            > 0
    })
}

/// The longest side the detail view asks the decoder for: enough for a 4K screen at 1:1 without
/// holding an 8064×6048 frame in memory.
const DETAIL_SIDE: u32 = 2400;
/// Only a few full-size frames are worth keeping — one is ~7 MB of RGBA, and the user looks at one
/// file at a time.
const DETAIL_CAPACITY: usize = 4;

pub struct AppState {
    pub lib: Library,
    pub tokens: Tokens,
    /// A rejected command, shown in the status bar until the next command succeeds.
    pub notice: Option<String>,
    /// Text in the sidebar's add-folder field; it belongs to the UI, not the model.
    pub add_path: String,
    /// Event batches the control channel queued, one per frame. They reach egui through
    /// `RawInput` (`raw_input_hook` in `main.rs`, `RawInput` in a test harness): egui builds its
    /// pointer state from the raw input of a pass, so pushing events into `ctx.input_mut` mid-pass
    /// would only ever reach code that reads `events` directly, never a click.
    pending: VecDeque<Vec<Event>>,
    /// Frames drawn so far: a script waits for `frame` to pass the frame it acted in.
    pub frame: u64,
    /// Set by `ui.quit`; `eframe` closes the viewport in `control::handle`.
    pub quit: bool,
    /// Outcomes of `ui.screenshot` requests, newest last: `<path>: N bytes` or the failure.
    pub shots: Vec<String>,
    /// Widgets the last drawn frame registered. `control` answers `ui.widgets`/`ui.click` from
    /// this, so a test can read it without a `Context`.
    pub widgets: Vec<Widget>,
    /// The app's end of the control channel, when the window was started with `--control`.
    inbox: Option<Inbox>,
    /// The decode thread. Only the real window creates it, so a snapshot never depends on when a
    /// decode finishes.
    thumb_worker: Option<Worker>,
    /// Uploaded thumbnails, keyed by file.
    thumbs: HashMap<std::path::PathBuf, TextureHandle>,
    /// Files already asked for, so a frame never queues the same decode twice.
    thumb_asked: HashSet<std::path::PathBuf>,
    /// Files the loader refused (a video, an unknown format). They stay placeholders.
    thumb_failed: HashSet<std::path::PathBuf>,
    /// Where the folder list is persisted. `None` means "do not persist", which is what a test gets
    /// unless it sets a store path explicitly.
    store_path: Option<std::path::PathBuf>,
    /// The add-folder field owns the arrow keys while it is focused; the grid must not move the
    /// selection while someone types a path.
    path_focused: bool,
    /// A tile to bring into view (set by a keyboard move, cleared after `scroll_hold` frames).
    scroll_focus: Option<usize>,
    /// Canvas state for the single-image view: pan, plus what the last frame drew. Public because
    /// the geometry tests check where the picture was actually painted.
    pub canvas: CanvasState,
    /// The zoom the canvas last centred on; a change re-centres the picture. `Option<Option<_>>`
    /// because "fit" (`None`) is a value of its own.
    zoom_seen: Option<Option<f32>>,
    /// Full-size frames for the detail view, keyed by file.
    detail_textures: HashMap<std::path::PathBuf, TextureHandle>,
    /// Files already asked for at detail size, and files the loader refused at that size.
    detail_asked: HashSet<std::path::PathBuf>,
    detail_failed: HashSet<std::path::PathBuf>,
    /// Frames left to keep asking for the jump. Selecting the first file also brings the inspector
    /// up, which narrows the grid and resets the offset in that same frame, so one frame is not
    /// enough: two frames land the jump after the layout has settled.
    scroll_hold: u8,
}

impl AppState {
    pub fn new(folder: PathBuf) -> Self {
        AppState {
            thumb_worker: None,
            thumbs: HashMap::new(),
            thumb_asked: HashSet::new(),
            thumb_failed: HashSet::new(),
            lib: Library::new(&folder),
            tokens: Tokens::photos(),
            notice: None,
            add_path: String::new(),
            pending: VecDeque::new(),
            shots: Vec::new(),
            widgets: Vec::new(),
            frame: 0,
            quit: false,
            inbox: None,
            store_path: None,
            path_focused: false,
            scroll_focus: None,
            scroll_hold: 0,
            canvas: CanvasState::default(),
            zoom_seen: None,
            detail_textures: HashMap::new(),
            detail_asked: HashSet::new(),
            detail_failed: HashSet::new(),
        }
    }

    /// Queue an input batch for the next frame(s). The control channel uses this instead of
    /// synthesising OS input, so an injected click goes through the same path as a real one.
    pub fn queue_events(&mut self, batch: Vec<Event>) {
        self.pending.push_back(batch);
    }

    /// The next queued batch, for the integration to put in `RawInput` before the pass starts.
    pub fn take_events(&mut self) -> Vec<Event> {
        self.pending.pop_front().unwrap_or_default()
    }

    /// Wire the control channel (`--control <port>`). Tests drive [`control::handle`] directly
    /// instead, so they need no socket and no window.
    /// Attach the thumbnail decoder. Only `main.rs` does this: a test that wants thumbnails
    /// attaches one explicitly, and snapshots must not depend on decode timing.
    pub fn set_thumb_worker(&mut self, worker: Worker) {
        self.thumb_worker = Some(worker);
    }

    /// The text currently in the add-folder field. Read-only: tests and the control channel need to
    /// tell "typed but not submitted" from "submitted".
    pub fn add_path(&self) -> &str {
        &self.add_path
    }

    /// Point the folder list at a file. `None` disables persistence (the test default).
    pub fn set_store(&mut self, path: Option<std::path::PathBuf>) {
        self.store_path = path;
    }

    /// The list index of a folder path, or the active folder when the path is not listed.
    pub fn folder_index(&self, path: &std::path::Path) -> usize {
        self.lib
            .folders
            .iter()
            .position(|slot| slot.path == path)
            .unwrap_or(self.lib.active)
    }

    pub fn store_path(&self) -> Option<&std::path::Path> {
        self.store_path.as_deref()
    }

    /// Read the persisted folder list. A folder that is no longer a directory is dropped from the
    /// list and named in the notice, so an unmounted volume shows as a message rather than an empty
    /// folder. An empty store leaves the current list alone: the first run keeps whatever the
    /// command line handed us.
    pub fn restore_store(&mut self) -> Result<spectrum_core::Saved, String> {
        let Some(path) = self.store_path.clone() else {
            return Ok(spectrum_core::Saved::default());
        };
        let saved = spectrum_core::load_library_store(&path)?;
        if saved.folders.is_empty() {
            return Ok(saved);
        }
        spectrum_core::apply_library_store(&mut self.lib, &saved);
        if !saved.dropped.is_empty() {
            let names: Vec<String> = saved
                .dropped
                .iter()
                .take(3)
                .map(|p| p.display().to_string())
                .collect();
            let more = saved.dropped.len().saturating_sub(names.len()).to_string();
            self.notice = Some(format!(
                "Skipped {} folder no longer on disk: {}{}",
                saved.dropped.len(),
                names.join(", "),
                if saved.dropped.len() > names.len() {
                    format!(" (+{more})")
                } else {
                    String::new()
                }
            ));
        }
        Ok(saved)
    }

    /// Persist the folder list. Returns the error text when the write failed, so the caller can put
    /// it in the status bar instead of losing the change silently.
    pub fn save_store(&mut self) -> Option<String> {
        let path = self.store_path.clone()?;
        spectrum_core::save_library_store(&path, &self.lib.folders, self.lib.active).err()
    }

    pub fn set_control(&mut self, inbox: Inbox) {
        self.inbox = Some(inbox);
    }

    pub fn run(&mut self, id: &str, args: Value) -> bool {
        match dispatch(&mut self.lib, id, &args) {
            Ok(_) => {
                self.notice = None;
                // The folder list is the one thing that has to survive a restart, so a change to it
                // is written as it happens rather than on quit.
                if id.starts_with("folder.") {
                    if let Some(err) = self.save_store() {
                        self.notice = Some(err);
                    }
                }
                true
            }
            Err(err) => {
                self.notice = Some(err.to_string());
                false
            }
        }
    }

    /// Clicks collected while drawing, dispatched after the frame's widgets are built so the
    /// model is not mutated while it is being read.
    fn click_tile(&self, ui: &Ui, index: usize, out: &mut Vec<(usize, bool, bool)>) {
        let modifiers = ui.input(|i| i.modifiers);
        out.push((index, modifiers.ctrl || modifiers.command, modifiers.shift));
    }

    /// Everything but drawing: answer control requests, deliver one queued event batch, and drop
    /// last frame's widget registry. Kept apart from [`Self::ui`] so logic can be driven without a
    /// paint pass, and the paint pass without logic.
    pub fn logic(&mut self, ctx: &Context) {
        if let Some(inbox) = self.inbox.as_mut() {
            for req in inbox.drain() {
                let reply = control::handle(self, Some(ctx), &req);
                let _ = req.reply.send(reply);
            }
        }
        widgets::begin_frame(ctx);
        self.frame += 1;
        self.pump_thumbs(ctx);
    }

    /// Upload finished decodes, then ask for what is on screen but not decoded yet. Three new
    /// requests per frame, because one HEIC decode here takes ~850 ms and queuing 32 at once would
    /// pile up work nobody has looked at yet.
    fn pump_thumbs(&mut self, ctx: &Context) {
        let finished = self.thumb_worker.as_ref().map(|worker| worker.take());
        for job in finished.unwrap_or_default() {
            let full_size = job.max >= DETAIL_SIDE;
            match job.rgba {
                Some((width, height, rgba)) => {
                    // Grid cells are square, so the centre is cropped: a 4:3 photo loses its sides
                    // instead of being squashed. The detail view wants the whole frame, which is why
                    // this is done here and not inside the decoder.
                    let (w, h, pixels) = if full_size {
                        (width, height, rgba)
                    } else {
                        let square = spectrum_core::thumbnail::crop_center_square(
                            &spectrum_core::thumbnail::Decoded {
                                width,
                                height,
                                rgba,
                            },
                        );
                        (square.width, square.height, square.rgba)
                    };
                    let image =
                        ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &pixels);
                    let name = if full_size {
                        format!("detail {}", job.path.to_string_lossy())
                    } else {
                        job.path.to_string_lossy().into_owned()
                    };
                    let texture = ctx.load_texture(name, image, TextureOptions::LINEAR);
                    if full_size {
                        self.detail_failed.remove(&job.path);
                        self.detail_textures.insert(job.path, texture);
                    } else {
                        self.thumb_failed.remove(&job.path);
                        self.thumbs.insert(job.path, texture);
                    }
                }
                None => {
                    if full_size {
                        self.detail_failed.insert(job.path);
                    } else {
                        self.thumb_failed.insert(job.path);
                    }
                }
            }
        }

        // Grid tiles: three per frame, because one HEIC costs ~850 ms and queuing everything would
        // pile up work nobody has looked at yet.
        let mut missing: Vec<std::path::PathBuf> = Vec::new();
        for index in self.lib.visible() {
            let path = self.lib.entries[index].path.clone();
            if self.thumbs.contains_key(&path)
                || self.thumb_failed.contains(&path)
                || self.thumb_asked.contains(&path)
            {
                continue;
            }
            missing.push(path);
            if missing.len() >= 3 {
                break;
            }
        }
        if let Some(worker) = self.thumb_worker.as_ref() {
            for path in &missing {
                worker.request(path.clone());
            }
        }
        // Without a worker (a snapshot test) the paths are still recorded, so nothing is asked for
        // again on a frame that can never be answered.
        self.thumb_asked.extend(missing);

        // The detail view asks for the open file once, at full size.
        if let Some(index) = self.lib.detail_index() {
            let path = self.lib.entries[index].path.clone();
            if !self.detail_textures.contains_key(&path)
                && !self.detail_failed.contains(&path)
                && !self.detail_asked.contains(&path)
            {
                if let Some(worker) = self.thumb_worker.as_ref() {
                    worker.request_at(path.clone(), DETAIL_SIDE, DETAIL_CAPACITY);
                }
                self.detail_asked.insert(path);
            }
        }
    }

    /// The decoded full-size frame for the open file, cloned out of the map so the canvas can be
    /// borrowed mutably while it is drawn.
    fn detail_texture(&self) -> Option<TextureHandle> {
        let index = self.lib.detail_index()?;
        self.lib
            .entries
            .get(index)
            .and_then(|entry| self.detail_textures.get(&entry.path))
            .cloned()
    }

    /// The decoded thumbnail for an entry, if there is one.
    fn thumbnail(&self, index: usize) -> Option<&TextureHandle> {
        self.lib
            .entries
            .get(index)
            .and_then(|e| self.thumbs.get(&e.path))
    }

    /// The one-call path a test harness uses: think, then draw. `eframe` calls the two halves
    /// separately (`App::logic` before the pass, `App::ui` during it).
    pub fn render(&mut self, ui: &mut Ui) {
        self.logic(ui.ctx());
        self.ui(ui);
    }

    /// Whether the detail view has a decoded frame yet. Tests wait on this rather than guessing a
    /// delay, because the decode happens on the worker thread.
    pub fn has_detail_texture(&self) -> bool {
        self.detail_texture().is_some()
    }

    /// Write back any screenshot egui handed us. The request comes from the control channel and
    /// the image comes back as an event, so this is egui's own capture of the real window — not a
    /// re-implementation of the layout.
    fn write_screenshots(&mut self, ui: &Ui) {
        let shots: Vec<(std::sync::Arc<ColorImage>, String)> = ui.input(|i| {
            i.events
                .iter()
                .filter_map(|event| match event {
                    Event::Screenshot {
                        image, user_data, ..
                    } => user_data
                        .data
                        .as_ref()
                        .and_then(|data| data.downcast_ref::<String>())
                        .map(|path| (std::sync::Arc::clone(image), path.clone())),
                    _ => None,
                })
                .collect()
        });
        for (image, path) in shots {
            self.shots.push(write_bmp(&path, &image));
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        let tokens = self.tokens;
        theme::apply(ui.ctx());
        self.handle_detail_keys(ui);

        let titlebar = Panel::top("titlebar")
            .exact_size(size::TITLEBAR_HEIGHT)
            .frame(
                Frame::new()
                    .fill(tokens.titlebar)
                    .outer_margin(0)
                    .inner_margin(Margin::symmetric(
                        space::TITLEBAR_INLINE as i8,
                        space::TITLEBAR_INLINE as i8,
                    )),
            )
            .show(ui, |ui| {
                self.titlebar(ui, &tokens);
            });
        widgets::register(ui, "panel:titlebar", titlebar.response.rect, "panel");

        if self.lib.status.is_visible() || self.notice.is_some() {
            let status = Panel::bottom("statusbar")
                .frame(
                    Frame::new()
                        .fill(tokens.titlebar)
                        .outer_margin(0)
                        .inner_margin(Margin::symmetric(12, 6)),
                )
                .show(ui, |ui| {
                    let status = self.lib.status.text();
                    let text = match &self.notice {
                        Some(notice) => notice.clone(),
                        None => status,
                    };
                    ui.label(
                        RichText::new(text)
                            .size(text::META_SIZE)
                            .color(tokens.secondary),
                    );
                });
            widgets::register(ui, "panel:statusbar", status.response.rect, "panel");
        }

        let sidebar = Panel::left("sidebar")
            .exact_size(size::SIDEBAR_WIDTH)
            .frame(
                Frame::new()
                    .fill(tokens.sidebar)
                    .outer_margin(0)
                    .inner_margin(Margin::symmetric(
                        space::SIDEBAR_PADDING as i8,
                        space::SIDEBAR_PADDING as i8,
                    )),
            )
            .show(ui, |ui| {
                let scroll = ScrollArea::vertical()
                    // A stable id: the inspector appearing must not reset the grid's scroll state,
                    // which would throw away a keyboard "bring into view" jump in the same frame.
                    .id_salt("grid")
                    .auto_shrink([false, false])
                    .show(ui, |ui| self.folders(ui, &tokens));
                widgets::register(ui, "sidebar:scroll", scroll.inner_rect, "scroll");
            });
        widgets::register(ui, "panel:sidebar", sidebar.response.rect, "panel");

        // The design's library screen has no inspector column: `Main` runs to the window's right
        // edge. The edit controls appear only once something is selected — and hiding the column is
        // also what lets the grid be seven 164 px columns wide, as the design draws it.
        let inspector = if self.lib.selection.set.is_empty() {
            None
        } else {
            Some(
                Panel::right("inspector")
                    .exact_size(size::INSPECTOR_WIDTH)
                    .frame(
                        Frame::new()
                            .fill(tokens.detail)
                            .outer_margin(0)
                            .inner_margin(Margin::symmetric(
                                space::INSPECTOR_PADDING as i8,
                                space::INSPECTOR_PADDING as i8,
                            )),
                    )
                    .show(ui, |ui| {
                        let scroll = ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| self.inspector(ui, &tokens));
                        widgets::register(ui, "inspector:scroll", scroll.inner_rect, "scroll");
                    }),
            )
        };
        if let Some(inspector) = inspector {
            widgets::register(ui, "panel:inspector", inspector.response.rect, "panel");
        }

        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(tokens.window)
                    .inner_margin(Margin::symmetric(
                        space::MAIN_INLINE as i8,
                        space::MAIN_PADDING as i8,
                    )),
            )
            .show(ui, |ui| {
                // The detail view replaces the grid: the photo is the subject, so the toolbar and the
                // tiles step out of the way.
                if self.lib.detail_index().is_some() {
                    self.detail_canvas(ui, &tokens);
                    return;
                }
                // The filter/sort bar belongs to this column, not to the window: in the design the
                // sidebar starts right under the titlebar, and a full-width bar would push it down.
                let bar = ui.vertical(|ui| {
                    self.toolbar(ui, &tokens);
                    ui.response()
                });
                widgets::register(ui, "panel:toolbar", bar.response.rect, "panel");
                widgets::hairline(ui, &tokens);
                ui.add_space(space::GRID_GAP);

                // Measure the wrap width outside the scroll area: inside it the child ui may be
                // given as much width as its content wants, and then a wrapped row runs for
                // thousands of pixels instead of filling the panel like the design's grid does.
                let wrap_width = ui.available_width();
                let scroll = ScrollArea::vertical()
                    .auto_shrink([false, false])
                    // The design's grid starts at the column's padding edge; egui's default scroll
                    // content margin would push it 8 px further in and cost a column.
                    .content_margin(0.0)
                    .show(ui, |ui| match self.lib.view {
                        ViewMode::All => self.library_view(ui, &tokens, wrap_width),
                        ViewMode::Months => self.months_view(ui, &tokens, wrap_width),
                        ViewMode::Years => self.years_view(ui, &tokens, wrap_width),
                    });
                widgets::register(ui, "grid:scroll", scroll.inner_rect, "scroll");
            });

        self.write_screenshots(ui);
        // Publish what this frame drew, in the order it was drawn. `ui()` does it, not `render()`:
        // `eframe` only calls `ui()` for the drawing half, and an unpublished registry means the
        // control channel finds nothing to click.
        self.widgets = widgets::registry(ui.ctx());
    }

    fn titlebar(&mut self, ui: &mut Ui, tokens: &Tokens) {
        let count = self.lib.entries.len();
        // One row: the titlebar is 52 px, so a second row would overflow its panel and the
        // widgets would sit where the toolbar is, taking clicks meant for them.
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(folder_name(self.lib.folder()))
                    .size(text::TITLE_SIZE)
                    .color(tokens.text)
                    .strong(),
            );
            ui.label(
                RichText::new(format!("{count} items"))
                    .size(text::META_SIZE)
                    .color(tokens.secondary),
            );
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::control_button(ui, tokens, "button:rescan", "Rescan", false).clicked() {
                    self.run("library.rescan", json!({}));
                }
            });
        });
    }

    /// Filter and sort live in this strip; the sidebar is only the folder list.
    fn toolbar(&mut self, ui: &mut Ui, tokens: &Tokens) {
        ui.horizontal(|ui| {
            let view_labels: Vec<&'static str> = VIEWS.iter().map(|v| v.label()).collect();
            let selected_view = VIEWS.iter().position(|v| *v == self.lib.view);
            if let Some(i) =
                widgets::segmented(ui, tokens, "segment:view", &view_labels, selected_view)
            {
                self.run("view.set", json!({"view": VIEWS[i].arg()}));
            }
            ui.add_space(space::SECTION_GAP);

            let kinds: [&str; 3] = ["All", "Photos", "Videos"];
            let selected = match self.lib.filter.kind {
                None => Some(0),
                Some(MediaKind::Photo) => Some(1),
                Some(MediaKind::Video) => Some(2),
            };
            if let Some(i) = widgets::segmented(ui, tokens, "segment:kind", &kinds, selected) {
                let kind = match i {
                    0 => json!(null),
                    1 => json!("photo"),
                    _ => json!("video"),
                };
                self.run("filter.kind", json!({"kind": kind}));
            }
            let hdr_on = self.lib.filter.hdr_only;
            if widgets::control_button(ui, tokens, "button:filter-hdr", "HDR only", hdr_on)
                .clicked()
            {
                self.run("filter.hdr", json!({"on": !hdr_on}));
            }
            if widgets::control_button(ui, tokens, "button:filter-clear", "Clear filter", false)
                .clicked()
            {
                self.run("filter.clear", json!({}));
            }

            ui.add_space(space::SECTION_GAP);
            let sort_labels: Vec<&'static str> = SORTS.iter().map(|k| k.label()).collect();
            let selected_sort = SORTS.iter().position(|k| *k == self.lib.sort);
            if let Some(i) =
                widgets::segmented(ui, tokens, "segment:sort", &sort_labels, selected_sort)
            {
                self.run(
                    "sort.set",
                    json!({"key": SORTS[i].arg(), "direction": self.lib.direction.label()}),
                );
            }
            let reversed = self.lib.direction == SortDirection::Reversed;
            let order_label = if reversed {
                "Reversed"
            } else {
                "Natural order"
            };
            if widgets::control_button(ui, tokens, "button:sort-order", order_label, false)
                .clicked()
            {
                let next = if reversed {
                    SortDirection::Natural
                } else {
                    SortDirection::Reversed
                };
                self.run(
                    "sort.set",
                    json!({"key": self.lib.sort.arg(), "direction": next.label()}),
                );
            }
        });
    }

    /// The folder list: every folder the user added, the open one marked, and the controls to add
    /// or drop one. Dropping a folder never deletes its files.
    fn folders(&mut self, ui: &mut Ui, tokens: &Tokens) {
        widgets::section_header(ui, tokens, "Folders");
        let slots: Vec<(String, usize, bool)> = self
            .lib
            .folders
            .iter()
            .enumerate()
            .map(|(index, slot)| {
                let label = match slot.count {
                    Some(count) => format!("{}  {count}", slot.label()),
                    None => slot.label(),
                };
                (label, index, index == self.lib.active)
            })
            .collect();
        for (label, index, on) in slots {
            let row = widgets::sidebar_row(ui, tokens, &format!("folder:{index}"), &label, on);
            if row.clicked() {
                self.run("folder.open", json!({"index": index}));
            }
        }
        widgets::group_gap(ui);

        let typed = ui.add(
            egui::TextEdit::singleline(&mut self.add_path)
                .hint_text("Folder path")
                .font(theme::font(text::CONTROL_SIZE))
                // egui's default `return_key` is Enter, and it *consumes* that key, so the Enter
                // check below would never see it. Enter is handled here instead, gated on focus.
                .return_key(None),
        );
        ui.add_space(space::CONTROL_INLINE);
        widgets::register(ui, "field:add-path", typed.rect, "field");
        // Track focus so the grid's arrow keys stay put while a path is being typed.
        self.path_focused = typed.has_focus();
        let path = self.add_path.trim().to_string();
        let add = widgets::control_button(
            ui,
            tokens,
            "button:add-folder",
            "Add folder",
            !path.is_empty(),
        );
        // Enter submits the path. This screen has exactly one text input, so Enter is not gated on
        // focus; if a second input is added (crop fields, search), gate this on the field's focus.
        // Enter submits the path, but only while the field holds focus: Enter elsewhere must not add
        // whatever was typed. Clicking the field only focuses it — it used to add the folder too, so
        // clicking to type a path silently changed the library.
        let enter_pressed =
            self.path_focused && !path.is_empty() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let wants_add = add.clicked() || enter_pressed;
        if wants_add && !path.is_empty() && self.run("folder.add", json!({"path": path})) {
            self.add_path.clear();
        }
        ui.add_space(space::CONTROL_INLINE);
        if widgets::control_button(ui, tokens, "button:remove-folder", "Remove folder", false)
            .clicked()
        {
            self.run("folder.remove", json!({"index": self.lib.active}));
        }
    }

    /// Library grid: month groups, collapsed ones skipped, tiles inside.
    fn library_view(&mut self, ui: &mut Ui, tokens: &Tokens, wrap_width: f32) {
        let groups: Vec<(String, Vec<usize>)> = self
            .lib
            .groups()
            .into_iter()
            .map(|group| {
                let collapsed = self.lib.collapsed.iter().any(|key| key == &group.key);
                (
                    group.key,
                    if collapsed { Vec::new() } else { group.indices },
                )
            })
            .collect();

        let mut clicks: Vec<(usize, bool, bool)> = Vec::new();
        for (key, indices) in groups {
            if indices.is_empty() {
                continue;
            }
            // The design's section header: 17 px semibold dark title, then the count in grey.
            let title = section_title(&key);
            let count = count_label(&self.lib.entries, &indices);
            let label = ui.horizontal(|ui| {
                let title = ui.add(
                    egui::Label::new(
                        RichText::new(title)
                            .size(text::TITLE_SIZE)
                            .color(tokens.text)
                            .strong(),
                    )
                    .sense(egui::Sense::click()),
                );
                ui.label(
                    RichText::new(count)
                        .size(text::CAPTION_SIZE)
                        .color(tokens.secondary),
                );
                title
            });
            widgets::register(ui, &format!("group:{key}"), label.response.rect, "row");
            if label.response.clicked() {
                self.run("group.toggle", json!({"key": key}));
            }
            ui.add_space(space::GRID_GAP);
            self.tiles(ui, tokens, &indices, wrap_width, &mut clicks);
            ui.add_space(space::SECTION_GAP);
        }
        self.dispatch_clicks(clicks);
    }

    fn months_view(&mut self, ui: &mut Ui, tokens: &Tokens, wrap_width: f32) {
        let groups = self.lib.groups();
        let mut clicks: Vec<(usize, bool, bool)> = Vec::new();
        for group in groups {
            let open = self.lib.filter.month.as_deref() == Some(group.key.as_str());
            let heading = format!(
                "{}  ·  {}",
                section_title(&group.key),
                count_label(&self.lib.entries, &group.indices)
            );
            let row =
                widgets::sidebar_row(ui, tokens, &format!("month:{}", group.key), &heading, open);
            if row.clicked() {
                self.run("month.open", json!({"key": group.key}));
            }
            if open {
                ui.add_space(space::GRID_GAP);
                self.tiles(ui, tokens, &group.indices, wrap_width, &mut clicks);
            }
            ui.add_space(space::SIDEBAR_GAP);
        }
        self.dispatch_clicks(clicks);
    }

    fn years_view(&mut self, ui: &mut Ui, tokens: &Tokens, wrap_width: f32) {
        let years: Vec<(String, Vec<usize>)> = self.lib.years();
        let mut clicks: Vec<(usize, bool, bool)> = Vec::new();
        for (key, indices) in years {
            let year = year_of(&key);
            let open = self.lib.year.as_deref() == Some(year.as_str());
            let heading = format!("{year}  ·  {}", count_label(&self.lib.entries, &indices));
            let row = widgets::sidebar_row(ui, tokens, &format!("year:{year}"), &heading, open);
            if row.clicked() {
                self.run("year.open", json!({"key": year}));
            }
            if open {
                ui.add_space(space::GRID_GAP);
                self.tiles(ui, tokens, &indices, wrap_width, &mut clicks);
            }
            ui.add_space(space::SIDEBAR_GAP);
        }
        self.dispatch_clicks(clicks);
    }

    /// Tiles at the design's 164 px cell with a 2 px gap; columns fit the available width.
    fn tiles(
        &mut self,
        ui: &mut Ui,
        tokens: &Tokens,
        indices: &[usize],
        wrap_width: f32,
        clicks: &mut Vec<(usize, bool, bool)>,
    ) {
        let gap = size::GRID_GAP;
        let columns = columns_for_width(wrap_width);
        // Keys are handled where the real column count is known, and before the model is borrowed
        // for drawing, so this frame already shows the moved selection.
        self.handle_nav_keys(ui, columns);
        let entries = &self.lib.entries;
        let selection = &self.lib.selection;
        // A wrapped row, not a `Grid`: `Grid` sizes its columns from whatever cells it has seen,
        // and with a folder of 32 items the trailing columns collapsed to the width of the HDR
        // badge, so tiles started ~122 px inside their neighbours.
        // Fixed columns, computed from the width the panel actually has. `horizontal_wrapped`
        // inside a vertical `ScrollArea` gets an unbounded child width, so a wrapped row ran for
        // thousands of pixels instead of filling the panel; a `Grid` instead sized its trailing
        // columns from the narrowest cell and let tiles overlap.
        ui.spacing_mut().item_spacing.y = gap;
        let pending_scroll = self.scroll_focus.filter(|_| self.scroll_hold > 0);
        for row in indices.chunks(columns) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(gap, gap);
                for &index in row {
                    let name = entries[index].name();
                    let selected = selection.contains(index);
                    let badge = self.lib.mode_for(index).label();
                    let hdr = matches!(self.lib.mode_for(index), DisplayMode::Hdr);
                    let image = self.thumbnail(index);
                    let response = widgets::tile(
                        ui,
                        tokens,
                        &format!("tile:{name}"),
                        TileSpec {
                            label: &name,
                            badge,
                            badge_on: hdr,
                            selected,
                            image,
                        },
                    );
                    if response.clicked() {
                        self.click_tile(ui, index, clicks);
                    }
                    if pending_scroll == Some(index) {
                        // Bring the keyboard-selected tile into view: `scroll_hold` keeps asking for
                        // a couple of frames, then stops so the user's own scrolling is not fought.
                        // No animation — a key press should land on the item, and egui's animated
                        // jump needs real elapsed time a settled-frame test does not have.
                        ui.scroll_to_rect_animation(
                            response.rect,
                            None,
                            egui::style::ScrollAnimation::none(),
                        );
                    }
                }
            });
        }
        self.scroll_hold = self.scroll_hold.saturating_sub(1);
        if self.scroll_hold == 0 {
            self.scroll_focus = None;
        }
    }

    /// Enter opens the selected file, Escape closes the detail view (and clears the selection when
    /// there is nothing to close), Ctrl/⌘ + `+`/`-` zooms. These are handled outside the grid because
    /// the detail view draws no tiles, so the grid's own key handler would never run.
    fn handle_detail_keys(&mut self, ui: &Ui) {
        if self.path_focused {
            return;
        }
        if ui.input(|i| i.key_pressed(egui::Key::Enter)) && self.lib.sole().is_some() {
            self.run("detail.open", json!({}));
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            if self.lib.detail.is_some() {
                self.run("detail.close", json!({}));
            } else {
                self.run("select.clear", json!({}));
            }
        }
        if self.lib.detail.is_some() {
            // `count_and_consume_key` reads the modifiers off the key event itself: egui only
            // updates `i.modifiers` from `ModifiersChanged` events, so checking `i.modifiers` here
            // would miss Ctrl in tests and in some backends. `+` and `=` share a key, so both zoom
            // in, and the logical match ignores the Shift that `+` needs.
            if zoom_key(ui, egui::Key::Equals) || zoom_key(ui, egui::Key::Plus) {
                self.zoom_request("in");
            } else if zoom_key(ui, egui::Key::Minus) {
                self.zoom_request("out");
            }
        }
    }

    /// Ask the registry for a zoom change, carrying the scale the canvas is showing so a zoom from
    /// "fit" continues from the real fit scale rather than from 1:1.
    fn zoom_request(&mut self, action: &str) {
        let current = self.canvas.scale as f64;
        self.run("detail.zoom", json!({"action": action, "current": current}));
    }

    /// The single-image canvas: strip, then the frame at its own size.
    fn detail_canvas(&mut self, ui: &mut Ui, tokens: &Tokens) {
        let zoom = self.lib.detail_zoom;
        // A zoom change re-centres: keeping the old pan while scaling leaves the picture somewhere
        // off screen and reads as a bug.
        if self.zoom_seen != Some(zoom) {
            self.zoom_seen = Some(zoom);
            self.canvas.pan = egui::Vec2::ZERO;
        }
        let texture = self.detail_texture();
        let outcome = detail::draw(ui, tokens, texture.as_ref(), zoom, &mut self.canvas);
        for action in outcome.zoom_actions {
            self.run(
                "detail.zoom",
                json!({"action": action, "current": outcome.percent as f64 / 100.0}),
            );
        }
        if outcome.close_clicked {
            self.run("detail.close", json!({}));
        }
    }

    /// Arrow/Home/End move the selection through the visible list; Escape clears it. Skipped while
    /// the add-folder field is focused, so typing a path never moves the grid.
    fn handle_nav_keys(&mut self, ui: &Ui, columns: usize) {
        if self.path_focused {
            return;
        }
        let directions: Vec<&'static str> = ui.input(|i| {
            [
                (i.key_pressed(egui::Key::ArrowLeft), "left"),
                (i.key_pressed(egui::Key::ArrowRight), "right"),
                (i.key_pressed(egui::Key::ArrowUp), "up"),
                (i.key_pressed(egui::Key::ArrowDown), "down"),
                (i.key_pressed(egui::Key::Home), "home"),
                (i.key_pressed(egui::Key::End), "end"),
            ]
            .into_iter()
            .filter_map(|(pressed, direction)| pressed.then_some(direction))
            .collect()
        });
        for direction in directions {
            if self.run(
                "select.move",
                json!({"direction": direction, "columns": columns}),
            ) {
                self.scroll_focus = self.lib.sole();
                self.scroll_hold = 2;
            }
        }
    }

    fn dispatch_clicks(&mut self, clicks: Vec<(usize, bool, bool)>) {
        for (index, ctrl, shift) in clicks {
            self.run(
                "grid.click",
                json!({"index": index, "ctrl": ctrl, "shift": shift}),
            );
        }
    }

    fn inspector(&mut self, ui: &mut Ui, tokens: &Tokens) {
        let sole = self.lib.sole();
        ui.vertical_centered(|ui| match sole {
            Some(index) => {
                let entry = &self.lib.entries[index];
                widgets::caption(ui, tokens, &entry.name(), &meta_line(entry));
                widgets::group_gap(ui);
                widgets::badge(
                    ui,
                    tokens,
                    "badge:inspector",
                    self.lib.badge_for(index),
                    matches!(self.lib.mode_for(index), DisplayMode::Hdr),
                );
                widgets::group_gap(ui);

                let labels: Vec<&'static str> = TABS.iter().map(|t| t.label()).collect();
                let selected = TABS.iter().position(|t| *t == self.lib.tab);
                if let Some(i) = widgets::segmented(ui, tokens, "segment:tab", &labels, selected) {
                    self.run("inspector.tab", json!({"tab": TABS[i].arg()}));
                }
                widgets::group_gap(ui);

                if self.lib.tab == InspectorTab::Adjust {
                    if widgets::control_button(
                        ui,
                        tokens,
                        "button:hdr-toggle",
                        "HDR / SDR toggle",
                        self.lib.hdr_enabled,
                    )
                    .clicked()
                    {
                        self.run("hdr.toggle", json!({}));
                    }
                    ui.add_space(space::CONTROL_INLINE);
                    ui.horizontal(|ui| {
                        if widgets::control_button(
                            ui,
                            tokens,
                            "button:rotate-ccw",
                            "Rotate ⟲",
                            false,
                        )
                        .clicked()
                        {
                            self.run("edit.rotate", json!({"dir": -90}));
                        }
                        if widgets::control_button(
                            ui,
                            tokens,
                            "button:rotate-cw",
                            "Rotate ⟳",
                            false,
                        )
                        .clicked()
                        {
                            self.run("edit.rotate", json!({"dir": 90}));
                        }
                    });
                    ui.add_space(space::CONTROL_INLINE);
                    ui.horizontal(|ui| {
                        if widgets::control_button(
                            ui,
                            tokens,
                            "button:flip-h",
                            "Flip horizontal",
                            false,
                        )
                        .clicked()
                        {
                            self.run("edit.flipH", json!({}));
                        }
                        if widgets::control_button(ui, tokens, "button:restore", "Restore", false)
                            .clicked()
                        {
                            self.run("edit.restore", json!({}));
                        }
                    });
                    ui.add_space(space::CONTROL_INLINE);
                    if widgets::control_button(ui, tokens, "button:trash", "Trash selected", false)
                        .clicked()
                    {
                        self.run("photo.trash", json!({}));
                    }
                } else {
                    for (name, value) in info_rows(&self.lib, index) {
                        info_row(ui, tokens, name, value);
                    }
                }
            }
            None => {
                let label = if self.lib.selection.count() == 0 {
                    "No selection"
                } else {
                    "Multi-selection"
                };
                ui.label(
                    RichText::new(label)
                        .size(text::ROW_SIZE)
                        .color(tokens.muted),
                );
                widgets::group_gap(ui);
                if widgets::control_button(
                    ui,
                    tokens,
                    "button:hdr-toggle",
                    "HDR / SDR toggle",
                    self.lib.hdr_enabled,
                )
                .clicked()
                {
                    self.run("hdr.toggle", json!({}));
                }
                ui.add_space(space::CONTROL_INLINE);
                if widgets::control_button(
                    ui,
                    tokens,
                    "button:clear-selection",
                    "Clear selection",
                    false,
                )
                .clicked()
                {
                    self.run("library.clear", json!({}));
                }
                ui.add_space(space::CONTROL_INLINE);
                if widgets::control_button(ui, tokens, "button:trash", "Trash selected", false)
                    .clicked()
                {
                    self.run("photo.trash", json!({}));
                }
            }
        });
    }
}

/// Write an egui capture as an uncompressed 24-bit BMP: no image encoder, and the byte layout is
/// simple enough to diff by hand. BMP rows are bottom-up and BGR.
fn write_bmp(path: &str, image: &ColorImage) -> String {
    use std::io::Write;

    let [width, height] = image.size;
    let row = width * 3;
    let padded = (row + 3) & !3;
    let mut file = match std::fs::File::create(path) {
        Ok(file) => std::io::BufWriter::new(file),
        Err(err) => return format!("{path}: cannot write ({err})"),
    };

    let mut header = Vec::with_capacity(54);
    header.extend_from_slice(b"BM");
    header.extend_from_slice(&((54 + padded * height) as u32).to_le_bytes());
    header.extend_from_slice(&[0; 4]);
    header.extend_from_slice(&54u32.to_le_bytes());
    header.extend_from_slice(&40u32.to_le_bytes());
    header.extend_from_slice(&(width as u32).to_le_bytes());
    header.extend_from_slice(&(height as u32).to_le_bytes());
    header.extend_from_slice(&1u16.to_le_bytes());
    header.extend_from_slice(&24u16.to_le_bytes());
    header.extend_from_slice(&[0; 4]);
    header.extend_from_slice(&((padded * height) as u32).to_le_bytes());
    header.extend_from_slice(&[0; 8]);
    header.extend_from_slice(&[0; 8]);
    if file.write_all(&header).is_err() {
        return format!("{path}: cannot write the header");
    }

    for y in (0..height).rev() {
        let mut line = vec![0u8; padded];
        for x in 0..width {
            let rgba = image.pixels[y * width + x].to_array();
            line[x * 3..x * 3 + 3].copy_from_slice(&[rgba[2], rgba[1], rgba[0]]);
        }
        if file.write_all(&line).is_err() {
            return format!("{path}: cannot write row {y}");
        }
    }
    if file.flush().is_err() {
        return format!("{path}: cannot flush");
    }
    format!("{path}: {width}×{height} px")
}

fn info_row(ui: &mut Ui, tokens: &Tokens, name: &'static str, value: String) {
    let row = Frame::new()
        .fill(tokens.window)
        .corner_radius(radius::CONTROL)
        .inner_margin(Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(value)
                        .size(text::CONTROL_SIZE)
                        .color(tokens.label),
                );
                ui.label(
                    RichText::new(name)
                        .size(text::CONTROL_SIZE)
                        .color(tokens.secondary),
                );
            });
        });
    widgets::register(ui, &format!("info:{name}"), row.response.rect, "row");
}

/// Folder basename for the titlebar.
fn folder_name(folder: &Path) -> String {
    folder
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| folder.display().to_string())
}

/// `YYYY-MM` → `YYYY`; `unknown` has no year and stays as is.
/// How many `164 px tile + 2 px gap` cells fit in `width`. The design's `Main` is 1207 px wide
/// with 20 px padding, so its grid is seven columns; `grid_columns` tests the function against
/// those numbers.
pub fn columns_for_width(width: f32) -> usize {
    (((width + size::GRID_GAP) / (size::TILE_SIZE + size::GRID_GAP))
        .floor()
        .max(1.0)) as usize
}

const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// `2016-08` → `August 2016`, `2016` → `2016`. The design writes month names, not ISO keys.
fn section_title(key: &str) -> String {
    let mut parts = key.split('-');
    match (parts.next(), parts.next()) {
        (Some(year), Some(month)) => match month.parse::<usize>() {
            Ok(n) if (1..=12).contains(&n) => format!("{} {}", MONTH_NAMES[n - 1], year),
            _ => key.to_string(),
        },
        (Some(year), None) => year.to_string(),
        (None, _) => key.to_string(),
    }
}

/// `12 Photos`, `3 Videos`, or `12 Items` when both kinds are in the section.
fn count_label(entries: &[MediaEntry], indices: &[usize]) -> String {
    let photos = indices
        .iter()
        .filter(|&&i| entries[i].kind == MediaKind::Photo)
        .count();
    let noun = if photos == indices.len() {
        "Photos"
    } else if photos == 0 {
        "Videos"
    } else {
        "Items"
    };
    format!("{} {noun}", indices.len())
}

fn year_of(key: &str) -> String {
    if key.len() >= 4 && key.chars().take(4).all(|c| c.is_ascii_digit()) {
        key[..4].to_string()
    } else {
        key.to_string()
    }
}

fn meta_line(entry: &MediaEntry) -> String {
    let mut parts = vec![entry.kind.label().to_string(), human_bytes(entry.size)];
    if let Some(mtime) = entry.mtime {
        parts.push(date_of(mtime));
    }
    parts.join("  ·  ")
}

fn info_rows(lib: &Library, index: usize) -> Vec<(&'static str, String)> {
    let entry = &lib.entries[index];
    let mut rows = vec![
        ("Kind", entry.kind.label().to_string()),
        ("Size", human_bytes(entry.size)),
        (
            "Modified",
            entry.mtime.map(date_of).unwrap_or_else(|| "unknown".into()),
        ),
        ("Display", lib.mode_for(index).label().to_string()),
        (
            "Edit",
            if entry.edit.is_some() {
                "sidecar"
            } else {
                "none"
            }
            .to_string(),
        ),
    ];
    if let Some(signal) = entry.signal.as_ref() {
        rows.push(("Transfer", signal.transfer.name().to_string()));
        rows.push(("Signal source", format!("{:?}", signal.source)));
        if let Some((num, den)) = signal.headroom {
            rows.push(("Gain map", format!("{num}/{den}")));
        }
    }
    if let Some(video) = entry.video.as_ref() {
        rows.push(("Video", video.describe()));
    }
    rows.push((
        "Position",
        format!("{} of {}", index + 1, lib.entries.len()),
    ));
    rows
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

/// `SystemTime` → `YYYY-MM-DD HH:MM` UTC, using the core's calendar helper.
fn date_of(time: SystemTime) -> String {
    let secs = match time.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(err) => -(err.duration().as_secs() as i64),
    };
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let rem = secs.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}",
        rem / 3600,
        (rem % 3600) / 60
    )
}
