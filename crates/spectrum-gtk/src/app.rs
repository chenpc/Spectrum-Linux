use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::SystemTime;

use gdk4 as gdk;
use glib as glib;
use gio as gio;
use libglycin_rebind as gly;
use libglycin_gtk4_rebind as gly_gtk;
use gtk4 as gtk;
use gtk4::prelude::*;
use crate::edit::apply_edit;
use spectrum_core::cover::{cover_label, sort_by_cover};
use spectrum_core::folder_cache::FolderListCache;
use spectrum_core::edit::{CompositeEdit, EditOp};
use spectrum_core::undo::{Change, UndoStack};
use spectrum_core::hdr::{badge_label, TransferFunction};
use spectrum_core::import::{ImportModel, ImportRow};
use spectrum_core::scan::{MediaEntry, MediaKind, scan_folder};
use spectrum_core::search::{normalize_query, search, SearchResult};
use spectrum_core::trash::{delete_paths, iso8601_utc, trash_root, DeleteOutcome};
use spectrum_core::clipboard::{paste, rename_dir, set_clipboard, Clipboard, ClipboardMode};
use spectrum_core::select::{apply_click, ClickMods, Selection};
use spectrum_core::timeline::{group_by_month, group_label};
use spectrum_core::sort::{SortDirection, SortKey};
use spectrum_core::status_bar::StatusBarModel;
use spectrum_core::sort::sort_indices;
use spectrum_core::filter::{filter_entries, Filter};
use spectrum_core::nav::{next_index, Direction};
use spectrum_core::thumbnail::ThumbnailService;
use spectrum_core::xmp;

pub struct State {
    pub folder: PathBuf,
    pub entries: Vec<MediaEntry>,
    /// Indices into `entries` in display order (filter applied, then sorted).
    pub display: Vec<usize>,
    /// Position in `display`.
    pub selected: Option<usize>,
    pub hdr_enabled: bool,
    /// Non-destructive edit ops, in the order the user applied them.
    pub ops: Vec<EditOp>,
    pub sort: SortKey,
    pub sort_dir: SortDirection,
    pub filter: Filter,
}

/// The entry shown at display position `pos` (None when it is filtered out).
pub fn entry_at(state: &State, pos: usize) -> Option<&MediaEntry> {
    state.display.get(pos).map(|&i| &state.entries[i])
}

#[derive(Clone)]
pub struct Ui {
    pub list: gtk::ListBox,
    /// Direct subfolders of `State::folder`, one row per folder with its cover file.
    pub subfolders: gtk::ListBox,
    /// Row index -> subfolder path, so a row click can resolve the folder without parsing the label.
    pub subfolder_paths: Rc<RefCell<Vec<PathBuf>>>,
    /// macOS `FolderListCache`: subfolder listings are kept in memory and reused until invalidated,
    /// so a directory scan is not repeated for every view refresh.
    pub cache: Rc<RefCell<FolderListCache>>,
    /// Filename + folder-name search box (macOS `.searchable`).
    pub search: gtk::Entry,
    pub results: gtk::ListBox,
    /// Row index -> (path, is_folder) for the search results.
    pub result_rows: Rc<RefCell<Vec<(PathBuf, bool)>>>,
    pub results_scroll: gtk::ScrolledWindow,
    pub sub_scroll: gtk::ScrolledWindow,
    /// Timeline: one row per month (newest first), plus a leading "all months" row.
    pub timeline: gtk::ListBox,
    /// Row index -> month key ("" = all months).
    pub timeline_months: Rc<RefCell<Vec<String>>>,
    pub timeline_scroll: gtk::ScrolledWindow,
    /// Import panel: media files grouped by day (newest first), one group header per day.
    pub import_list: gtk::ListBox,
    pub import_scroll: gtk::ScrolledWindow,
    pub import_model: Rc<RefCell<ImportModel>>,
    /// Row index -> the model row it renders (header or item).
    pub import_rows: Rc<RefCell<Vec<ImportRow>>>,
    pub import_expand: gtk::Button,
    pub import_collapse: gtk::Button,
    pub multi: Rc<RefCell<Selection>>,
    pub mods: Rc<RefCell<ClickMods>>,
    pub grid: gtk::GridView,
    pub badge: gtk::Label,
    pub detail: gtk::Label,
    /// EXIF camera/lens summary for the selected file.
    pub info: gtk::Label,
    pub picture: gtk::Picture,
    pub toggle: gtk::Button,
    pub rotate: gtk::Button,
    pub flip: gtk::Button,
    pub save: gtk::Button,
    pub delete: gtk::Button,
    pub copy: gtk::Button,
    pub cut: gtk::Button,
    pub paste: gtk::Button,
    pub rename: gtk::Button,
    pub rename_entry: gtk::Entry,
    pub clipboard: Rc<RefCell<Clipboard>>,
    /// Task progress for scan/copy/delete (macOS DESIGN.md §4.11).
    pub bar: Rc<RefCell<StatusBarModel>>,
    pub progress: gtk::Label,
    /// Injectable clock: the completion message TTL is a deadline, so the e2e test can advance it
    /// deterministically instead of sleeping 10 seconds.
    pub clock: Rc<RefCell<SystemTime>>,
    /// Edit history (undo/redo), shared across files. A new edit clears the redo branch.
    pub history: Rc<RefCell<UndoStack>>,
    pub undo: gtk::Button,
    pub redo: gtk::Button,
    pub selection: gtk::MultiSelection,
    /// Guards the mirror: programmatic `select_item` calls emit `selection-changed`, and
    /// the handler must not treat those as user clicks.
    pub syncing: Rc<RefCell<bool>>,
    pub item_count: Rc<RefCell<usize>>,
    pub store: gio::ListStore,
    pub sort_box: gtk::DropDown,
    pub reverse: gtk::Button,
    pub filter_kind: gtk::DropDown,
    pub filter_hdr_only: gtk::CheckButton,
    pub transfer_box: gtk::DropDown,
    /// `{visible} of {total} · {filter} · sorted by {sort}`
    pub status: gtk::Label,

    pub thumbs: Rc<RefCell<ThumbnailService>>,
    /// Labels created by the list/grid factories as `(path, label)` pairs. A file can have one
    /// label in the list and one in the grid, so the HDR/SDR toggle must update every one.
    pub labels: Rc<RefCell<Vec<(PathBuf, gtk::Label)>>>,
    /// Thumbnails created by the list/grid factories as `(path, picture)` pairs, so an edit can be
    /// re-applied to every visible thumbnail when the ops change.
    pub thumbs_pics: Rc<RefCell<Vec<(PathBuf, gtk::Picture)>>>,
}

pub struct App {
    pub window: gtk::Window,
    pub state: Rc<RefCell<State>>,
    pub ui: Ui,
}


/// Rebuild the model contents in `state.entries` order (the display order).
pub fn rebuild_store(state: &Rc<RefCell<State>>, ui: &Ui) {
    // Clone first: `append` emits `items-changed`, which can fire `selected-notify` synchronously,
    // and that handler borrows the state. Holding a borrow across it panics.
    let entries: Vec<MediaEntry> = {
        let st = state.borrow();
        st.display.iter().map(|&i| st.entries[i].clone()).collect()
    };
    ui.store.remove_all();
    for e in &entries {
        ui.store.append(&glib::BoxedAnyObject::new(e.clone()));
    }
    *ui.item_count.borrow_mut() = entries.len();
}

/// Rebuild the display order (filter, then sort) and keep `selected` on the same file when it is
/// still visible.
///
/// Takes the RefCell (not `&mut State`) because rebuilding the model emits `items-changed`, which
/// can fire `selected-notify` synchronously; holding a borrow across it panics.
/// Move the selection with a grid navigation direction.
///
/// GTK4 has no in-process key injection, so the key controller is only a thin wrapper around
/// `nav::next_index`; keeping this separate is what makes navigation testable.
pub fn move_selection(state: &Rc<RefCell<State>>, ui: &Ui, dir: Direction) -> Option<usize> {
    let (n, cur, columns) = {
        let st = state.borrow();
        (
            st.display.len(),
            st.selected.unwrap_or(0),
            ui.grid.min_columns().max(1) as usize,
        )
    };
    let next = next_index(cur, n, columns, dir);
    if let Some(p) = next {
        record_click(state, ui, p, ClickMods::default());
    }
    next
}

pub fn apply_view(state: &Rc<RefCell<State>>, ui: &Ui) {
    let (order, keep, st_entries) = {
        let st = state.borrow();
        let idx = filter_entries(&st.entries, &st.filter);
        let order = sort_indices(&st.entries, st.sort, &idx, st.sort_dir);
        let keep = st
            .selected
            .and_then(|p| st.display.get(p))
            .map(|&i| st.entries[i].path.clone());
        (order, keep, st.entries.clone())
    };
    // If the selected file is filtered out, fall back to the first visible one so the selection
    // stays valid. Capture the position before `rebuild_store`: appending items can clamp the
    // SingleSelection to 0 and fire selected-notify, which would overwrite it.
    let pos = keep
        .as_ref()
        .and_then(|p| order.iter().position(|&i| &st_entries[i].path == p))
        // `order` holds entry indices, so `order.first()` is not a display position; the fallback
        // must be position 0 (the first visible row), not the entry index of the first file.
        .or_else(|| if order.is_empty() { None } else { Some(0) });
    {
        let st = &mut *state.borrow_mut();
        st.display = order;
        st.selected = pos;
    }
    rebuild_store(state, ui);
    if let Some(p) = pos {
        record_click(state, ui, p, ClickMods::default());
    }
    rebuild_timeline(state, ui);
    refresh(&state.borrow(), ui);
}

/// Rebuild the month rows for the current display order. Row 0 is "all months" (clears the filter).
pub fn rebuild_timeline(state: &Rc<RefCell<State>>, ui: &Ui) {
    let (groups, visible) = {
        let st = state.borrow();
        (group_by_month(&st.entries, &st.display), st.display.len())
    };
    ui.timeline.remove_all();
    ui.timeline_months.borrow_mut().clear();
    append_timeline(ui, &format!("all months  ·  {visible}"), "");
    for g in &groups {
        append_timeline(ui, &group_label(g), &g.key);
    }
}

fn append_timeline(ui: &Ui, text: &str, key: &str) {
    let row = gtk::ListBoxRow::new();
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    row.set_child(Some(&label));
    row.set_activatable(true);
    ui.timeline.append(&row);
    ui.timeline_months.borrow_mut().push(key.to_string());
}

/// Apply a month filter (empty key = no month filter).
pub fn select_month(state: &Rc<RefCell<State>>, ui: &Ui, key: &str) {
    {
        let st = &mut *state.borrow_mut();
        st.filter.month = if key.is_empty() { None } else { Some(key.to_string()) };
    }
    apply_view(state, ui);
}

/// Rebuild the subfolder rows for `State::folder` (sorted by name) and the row-index -> path map.
/// Scan the current folder for the import panel. Expansion state is kept per group key so a
/// rescan does not collapse groups the user had opened.
pub fn rescan_import(state: &Rc<RefCell<State>>, ui: &Ui) {
    let folder = { state.borrow().folder.clone() };
    let kept: Vec<(String, bool)> = {
        let m = ui.import_model.borrow();
        m.groups.iter().map(|g| (g.key.clone(), g.expanded)).collect()
    };
    let mut model = ImportModel::scan(&folder);
    for g in model.groups.iter_mut() {
        if let Some((_, expanded)) = kept.iter().find(|(k, _)| k == &g.key) {
            g.expanded = *expanded;
        }
    }
    *ui.import_model.borrow_mut() = model;
    render_import_rows(ui);
}

/// Rebuild the import rows from the model: a header per day, then the items of every expanded group.
pub fn render_import_rows(ui: &Ui) {
    let rows = ui.import_model.borrow().rows();
    ui.import_list.remove_all();
    ui.import_rows.borrow_mut().clear();
    for kind in rows {
        let text = match kind {
            ImportRow::Header(g) => ui.import_model.borrow().group_label(g),
            ImportRow::Item(i) => ui.import_model.borrow().item_name(i),
        };
        let label = gtk::Label::new(Some(&text));
        label.set_xalign(0.0);
        let row = gtk::ListBoxRow::new();
        row.set_child(Some(&label));
        row.set_activatable(true);
        ui.import_list.append(&row);
        ui.import_rows.borrow_mut().push(kind);
    }
}

pub fn rebuild_subfolders(state: &Rc<RefCell<State>>, ui: &Ui) {
    let folder = { state.borrow().folder.clone() };
    // macOS `subfoldersAreInOrder`: newest cover date first, name on a tie, no-cover last. The listing
    // itself comes from the cache, so a refresh does not re-read the directory.
    let infos = ui.cache.borrow_mut().sorted(&folder);
    ui.subfolders.remove_all();
    paths_of(ui).clear();
    for info in &infos {
        let row = gtk::ListBoxRow::new();
        let label = gtk::Label::new(Some(&cover_label(info)));
        label.set_xalign(0.0);
        row.set_child(Some(&label));
        row.set_activatable(true);
        ui.subfolders.append(&row);
        paths_of(ui).push(info.path.clone());
    }
}

fn paths_of(ui: &Ui) -> std::cell::RefMut<'_, Vec<PathBuf>> {
    ui.subfolder_paths.borrow_mut()
}

/// Scan a subfolder and refresh the file list. Non-destructive edits are per-file (XMP sidecar), so
/// the ops list is cleared when the folder changes.
pub fn open_folder(state: &Rc<RefCell<State>>, ui: &Ui, path: &Path) {
    if state.borrow().folder == path {
        return;
    }
    {
        let mut st = state.borrow_mut();
        st.folder = path.to_path_buf();
        ui.bar.borrow_mut().begin("scanning");
        let scanned = scan_folder(path);
        ui.bar.borrow_mut().update(scanned.len(), "scanning");
        ui.bar
            .borrow_mut()
            .finish(&format!("scanned {}", scanned.len()), *ui.clock.borrow());
        st.entries = scanned;
        st.ops.clear();
        st.selected = None;
    }
    rebuild_subfolders(state, ui);
    rescan_import(state, ui);
    apply_view(state, ui);
}

/// Run the filename/folder-name search over `State::folder` and rebuild the result rows.
/// A query shorter than 2 characters shows nothing (macOS guard), and the results area replaces
/// the subfolder area while a query is active.
pub fn run_search(state: &Rc<RefCell<State>>, ui: &Ui, query: &str) -> SearchResult {
    ui.results.remove_all();
    ui.result_rows.borrow_mut().clear();
    let lower = match normalize_query(query) {
        Some(l) => l,
        None => {
            ui.results_scroll.set_visible(false);
            ui.sub_scroll.set_visible(true);
            return SearchResult::default();
        }
    };
    ui.results_scroll.set_visible(true);
    ui.sub_scroll.set_visible(false);
    let res = search(&state.borrow().folder, &lower);
    for (path, name) in &res.folders {
        append_result(ui, &format!("{name}  ·  folder"), path.clone(), true);
    }
    for hit in &res.photos {
        let kind = match hit.kind {
            MediaKind::Photo => "photo",
            MediaKind::Video => "video",
        };
        append_result(ui, &format!("{}  ·  {}", hit.name, kind), hit.path.clone(), false);
    }
    res
}

fn append_result(ui: &Ui, text: &str, path: PathBuf, is_folder: bool) {
    let row = gtk::ListBoxRow::new();
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    row.set_child(Some(&label));
    row.set_activatable(true);
    ui.results.append(&row);
    ui.result_rows.borrow_mut().push((path, is_folder));
}

/// Open the folder containing `path` and select that file - what clicking a search result does.
pub fn open_and_select(state: &Rc<RefCell<State>>, ui: &Ui, path: &Path) {
    let parent = match path.parent() {
        Some(p) => p.to_path_buf(),
        None => return,
    };
    open_folder(state, ui, &parent);
    let idx = { state.borrow().entries.iter().position(|e| e.path == path) };
    if let Some(i) = idx {
        record_click(state, ui, i, ClickMods::default());
    }
}

pub fn sort_key_from_index(i: u32) -> SortKey {
    match i {
        1 => SortKey::DateModified,
        2 => SortKey::Kind,
        3 => SortKey::Transfer,
        4 => SortKey::Size,
        _ => SortKey::Name,
    }
}

pub fn filter_kind_from_index(i: u32) -> Option<MediaKind> {
    match i {
        1 => Some(MediaKind::Photo),
        2 => Some(MediaKind::Video),
        _ => None,
    }
}

pub fn filter_transfer_from_index(i: u32) -> Option<TransferFunction> {
    match i {
        1 => Some(TransferFunction::Hlg),
        2 => Some(TransferFunction::Pq),
        _ => None,
    }
}

pub fn fixture_dir() -> PathBuf {
    PathBuf::from("/tmp/spectrum-e2e-fixtures")
}

pub fn decode_frame(path: &Path) -> Result<gly::Frame, String> {
    let file = gio::File::for_path(path);
    let loader = gly::Loader::new(&file);
    // Without this, EXIF orientation is ignored and the texture keeps the raw sensor size.
    loader.set_apply_transformations(true);
    let image = loader.load().map_err(|e| e.to_string())?;
    image.next_frame().map_err(|e| e.to_string())
}

pub fn decode_texture(path: &Path) -> Result<gdk::Texture, String> {
    let frame = decode_frame(path)?;
    Ok(gly_gtk::frame_get_texture(&frame))
}

pub fn edit_of(state: &State) -> CompositeEdit {
    CompositeEdit::from(&state.ops)
}

/// Live ops win; otherwise the edit stored in the file's XMP sidecar is applied.
pub fn effective_edit(state: &State, e: &MediaEntry) -> CompositeEdit {
    if !state.ops.is_empty() {
        CompositeEdit::from(&state.ops)
    } else {
        e.edit.as_ref().map(|s| s.edit()).unwrap_or_default()
    }
}

pub fn apply_op(state: &mut State, ui: &Ui, op: EditOp) {
    let path = state.selected.and_then(|i| entry_at(state, i)).map(|e| e.path.clone());
    let before = state.ops.clone();
    state.ops.push(op.clone());
    if let Some(p) = path {
        ui.history.borrow_mut().record(Change::new(p, before, op.clone()));
    }
    refresh(state, ui);
}

/// Restore the ops from the newest recorded change; the selection follows that file when visible.
pub fn undo_last(state: &mut State, ui: &Ui) -> Option<String> {
    let change = ui.history.borrow_mut().undo()?;
    state.ops = change.ops_before;
    select_path(state, ui, &change.path);
    refresh(state, ui);
    Some(format!("undo: {}", change.path.display()))
}

pub fn redo_next(state: &mut State, ui: &Ui) -> Option<String> {
    let change = ui.history.borrow_mut().redo()?;
    state.ops = change.ops_after;
    select_path(state, ui, &change.path);
    refresh(state, ui);
    Some(format!("redo: {}", change.path.display()))
}

/// Select the display position of `path` (no-op when the file is filtered out).
fn select_path(state: &mut State, ui: &Ui, path: &Path) {
    let Some(idx) = state.entries.iter().position(|e| e.path == path) else {
        return;
    };
    if let Some(pos) = state.display.iter().position(|&i| i == idx) {
        state.selected = Some(pos);
        *ui.multi.borrow_mut() = Selection::default();
        ui.multi.borrow_mut().primary = Some(idx);
        ui.multi.borrow_mut().set = vec![idx];
        mirror_model(ui, &ui.multi.borrow());
    }
}

/// Write the non-destructive edit to `{name}.{ext}.xmp` (same layout as the macOS reference).
pub fn save_sidecar(state: &State) -> Result<PathBuf, String> {
    let i = state.selected.ok_or("no selection".to_string())?;
    let e = entry_at(state, i).ok_or("selection is filtered out".to_string())?;
    let orientation = e.signal.as_ref().map(|s| s.orientation as i32).unwrap_or(1);
    xmp::write_sidecar(&e.path, &edit_of(state), orientation, None).map_err(|e| e.to_string())
}

/// Fill a thumbnail picture from the shared cache: the first request decodes, later ones are hits.
pub fn paint_thumbnail(ui: &Ui, state: &State, path: &Path, pic: &gtk::Picture) {
    let max = ui.thumbs.borrow().max();
    let edit = state
        .entries
        .iter()
        .find(|e| &e.path == path)
        .map(|e| effective_edit(state, e))
        .unwrap_or_default();

    match ui.thumbs.borrow_mut().get(path) {
        Ok((frame, _)) => {
            let tex = if edit.is_identity() {
                gly_gtk::frame_get_texture(&frame)
            } else {
                match apply_edit(&frame, &edit) {
                    Ok(t) => t,
                    Err(_) => return,
                }
            };
            let (w, h) = spectrum_core::thumbnail::scale_within(tex.width() as u32, tex.height() as u32, max);
            pic.set_size_request(w as i32, h as i32);
            pic.set_paintable(Some(&tex));
        }
        Err(_) => pic.set_paintable(None::<&gdk::Paintable>),
    }
}

fn thumb_size(kind: MediaKind) -> (i32, i32) {
    match kind {
        MediaKind::Photo => (48, 48),
        MediaKind::Video => (64, 64),
    }
}

/// A row/tile: thumbnail + "name · transfer function".
///
/// `ContentFit::ScaleDown` keeps the thumbnail at its natural size when the allocation is
/// larger, so small images are not upscaled. `set_size_request` gives the widget a stable box
/// (a widget with a zero allocation produces no render node, which empties the parent snapshot).
fn item_box(name: &str, transfer: &str, size: (i32, i32)) -> (gtk::Box, gtk::Picture, gtk::Label) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let pic = gtk::Picture::new();
    pic.set_size_request(size.0, size.1);
    pic.set_content_fit(gtk::ContentFit::ScaleDown);
    row.append(&pic);
    let label = gtk::Label::new(Some(&format!("{name}  ·  {transfer}")));
    label.set_xalign(0.0);
    row.append(&label);
    (row, pic, label)
}

impl App {
    pub fn new(folder: &Path) -> App {
        let entries = scan_folder(folder);
        let state = Rc::new(RefCell::new(State {
            folder: folder.to_path_buf(),
            entries: entries.clone(),
            display: (0..entries.len()).collect(),
            selected: None,
            hdr_enabled: true,
            ops: Vec::new(),
            sort: SortKey::Name,
            sort_dir: SortDirection::Natural,
            filter: Filter::default(),
        }));

        let window = gtk::Window::new();
        window.set_title(Some("Spectrum"));
        window.set_default_size(900, 520);

        let ui = Ui {
            list: gtk::ListBox::new(),
            subfolders: gtk::ListBox::new(),
            subfolder_paths: Rc::new(RefCell::new(Vec::new())),
            cache: Rc::new(RefCell::new(FolderListCache::new())),
            // gtk4-rs `new()` takes the model/factory by value (`Option<impl IsA<...>>`), so pass
            // None here and attach them with the by-reference setters below.
            grid: gtk::GridView::new(None::<gtk::SelectionModel>, None::<gtk::ListItemFactory>),
            badge: gtk::Label::new(Some("SDR")),
            detail: gtk::Label::new(Some("(no selection)")),
            info: gtk::Label::new(Some("(no EXIF)")),
            picture: gtk::Picture::new(),
            toggle: gtk::Button::with_label("HDR/SDR toggle"),
            delete: gtk::Button::with_label("Delete to trash"),
            copy: gtk::Button::with_label("Copy"),
            cut: gtk::Button::with_label("Cut"),
            paste: gtk::Button::with_label("Paste"),
            rename: gtk::Button::with_label("Rename folder"),
            rename_entry: gtk::Entry::new(),
            clipboard: Rc::new(RefCell::new(Clipboard::default())),
            bar: Rc::new(RefCell::new(StatusBarModel::new())),
            progress: gtk::Label::new(Some("")),
            clock: Rc::new(RefCell::new(SystemTime::now())),
            history: Rc::new(RefCell::new(UndoStack::new())),
            undo: gtk::Button::with_label("undo"),
            redo: gtk::Button::with_label("redo"),
            rotate: gtk::Button::with_label("Rotate 90°"),
            flip: gtk::Button::with_label("Flip H"),
            save: gtk::Button::with_label("Save sidecar"),
            selection: gtk::MultiSelection::new(None::<gio::ListModel>),
            syncing: Rc::new(RefCell::new(false)),
            item_count: Rc::new(RefCell::new(0)),
            store: gio::ListStore::new::<glib::BoxedAnyObject>(),
            sort_box: gtk::DropDown::from_strings(&["Name", "Date", "Kind", "HDR first", "Size"]),
            reverse: gtk::Button::with_label("reverse"),
            filter_kind: gtk::DropDown::from_strings(&["All kinds", "Photos", "Videos"]),
            filter_hdr_only: gtk::CheckButton::new(),
            transfer_box: gtk::DropDown::from_strings(&["Transfer: any", "HLG", "PQ"]),
            status: gtk::Label::new(Some("0 of 0")),

            thumbs: Rc::new(RefCell::new(ThumbnailService::with_capacity(64, 256))),
            search: gtk::Entry::new(),
            results: gtk::ListBox::new(),
            result_rows: Rc::new(RefCell::new(Vec::new())),
            results_scroll: gtk::ScrolledWindow::new(),
            sub_scroll: gtk::ScrolledWindow::new(),
            timeline: gtk::ListBox::new(),
            timeline_months: Rc::new(RefCell::new(Vec::new())),
            timeline_scroll: gtk::ScrolledWindow::new(),
            import_list: gtk::ListBox::new(),
            import_scroll: gtk::ScrolledWindow::new(),
            import_model: Rc::new(RefCell::new(ImportModel::default())),
            import_rows: Rc::new(RefCell::new(Vec::new())),
            import_expand: gtk::Button::with_label("expand all"),
            import_collapse: gtk::Button::with_label("collapse all"),
            multi: Rc::new(RefCell::new(Selection::default())),
            mods: Rc::new(RefCell::new(ClickMods::default())),
            labels: Rc::new(RefCell::new(Vec::new())),
            thumbs_pics: Rc::new(RefCell::new(Vec::new())),
        };
        ui.list.set_selection_mode(gtk::SelectionMode::Multiple);
        ui.list.set_vexpand(true);
        ui.subfolders.set_selection_mode(gtk::SelectionMode::Single);
        ui.results.set_selection_mode(gtk::SelectionMode::Single);
        ui.timeline.set_selection_mode(gtk::SelectionMode::Single);
        ui.results.set_vexpand(true);
        ui.grid.set_min_columns(3);
        ui.grid.set_max_columns(6);
        ui.grid.set_single_click_activate(true);
        ui.grid.set_vexpand(true);
        ui.picture.set_content_fit(gtk::ContentFit::Contain);
        ui.picture.set_hexpand(true);
        ui.picture.set_vexpand(true);
        ui.picture.set_size_request(200, 160);

        let model = ui.store.clone();
        for e in &entries {
            model.append(&glib::BoxedAnyObject::new(e.clone()));
        }
        // App::new appends directly (not through rebuild_store), so the mirror needs the count too.
        *ui.item_count.borrow_mut() = entries.len();
        ui.selection.set_model(Some(&model));
        ui.grid.set_model(Some(&ui.selection));

        // One selection model drives both the list and the grid, so they stay in sync.
        let st = Rc::clone(&state);
        let u = ui.clone();
        ui.selection.connect_selection_changed(move |_sel, position, _n_items| {
            if *u.syncing.borrow() {
                return;
            }
            let idx = position as usize;
            record_click(&st, &u, idx, *u.mods.borrow());
        });

        // The ListBox keeps its own model, so its highlight is mirrored from the core set; this
        // handler only records the row the user actually clicked.
        let st_row = Rc::clone(&state);
        let u_row = ui.clone();
        ui.list.connect_row_selected(move |_list, row| {
            if *u_row.syncing.borrow() {
                return;
            }
            let Some(r) = row else { return };
            let idx = r.index() as usize;
            record_click(&st_row, &u_row, idx, *u_row.mods.borrow());
        });

        let labels = Rc::clone(&ui.labels);
        let pics = Rc::clone(&ui.thumbs_pics);
        let st_labels = Rc::clone(&state);
        let u_list = ui.clone();
        ui.list.bind_model(Some(&model), move |item| {
            // ListBox.bind_model hands the model item directly (not a ListItem).
            let e = item.downcast_ref::<glib::BoxedAnyObject>().unwrap().borrow::<MediaEntry>();
            let hdr = st_labels.borrow().hdr_enabled;
            let (row, pic, label) = item_box(
                &e.name(),
                badge_label(e.signal.as_ref(), hdr),
                thumb_size(e.kind),
            );
            paint_thumbnail(&u_list, &st_labels.borrow(), &e.path, &pic);
            pics.borrow_mut().push((e.path.clone(), pic.clone()));
            labels.borrow_mut().push((e.path.clone(), label));
            row.upcast()
        });

        let factory = gtk::SignalListItemFactory::new();
        let labels_grid = Rc::clone(&ui.labels);
        let pics_grid = Rc::clone(&ui.thumbs_pics);
        let st_grid = Rc::clone(&state);
        let u_grid = ui.clone();
        // With the v4_8+ feature the setup/bind signals hand over `&glib::Object` (the ListItem).
        factory.connect_setup(move |_factory, obj| {
            let item = obj.downcast_ref::<gtk::ListItem>().unwrap();
            let tile = gtk::Box::new(gtk::Orientation::Vertical, 4);
            let pic = gtk::Picture::new();
            pic.set_size_request(64, 64);
            pic.set_content_fit(gtk::ContentFit::ScaleDown);
            tile.append(&pic);
            let label = gtk::Label::new(None);
            label.set_xalign(0.0);
            tile.append(&label);
            item.set_child(Some(&tile));
        });
        factory.connect_bind(move |_factory, obj| {
            let item = obj.downcast_ref::<gtk::ListItem>().unwrap();
            let obj = item.item().and_then(|o| o.downcast::<glib::BoxedAnyObject>().ok());
            let tile = item.child().and_then(|c| c.downcast::<gtk::Box>().ok());
            if let (Some(obj), Some(tile)) = (obj, tile) {
                let e = obj.borrow::<MediaEntry>();
                let label = tile.last_child().and_then(|c| c.downcast::<gtk::Label>().ok());
                let pic = tile.first_child().and_then(|c| c.downcast::<gtk::Picture>().ok());
                if let Some(label) = label {
                    let hdr = st_grid.borrow().hdr_enabled;
                    labels_grid.borrow_mut().push((e.path.clone(), label.clone()));
                    label.set_text(&format!(
                        "{}  ·  {}",
                        e.name(),
                        badge_label(e.signal.as_ref(), hdr)
                    ));
                }
                if let Some(pic) = pic {
                    paint_thumbnail(&u_grid, &st_grid.borrow(), &e.path, &pic);
                    pics_grid.borrow_mut().push((e.path.clone(), pic.clone()));
                }
            }
        });
        ui.grid.set_factory(Some(&factory));

        let st4 = Rc::clone(&state);
        let u4 = ui.clone();
        ui.toggle.connect_clicked(move |_| {
            // `borrow_mut()` and `borrow()` must not overlap: a RefCell panic inside a GLib
            // callback cannot unwind through the C frame and aborts the process.
            let st = &mut *st4.borrow_mut();
            st.hdr_enabled = !st.hdr_enabled;
            refresh(st, &u4);
        });

        for (button, op) in [
            (ui.rotate.clone(), EditOp::Rotate(90)),
            (ui.flip.clone(), EditOp::FlipH),
        ] {
            let st5 = Rc::clone(&state);
            let u5 = ui.clone();
            button.connect_clicked(move |_| {
                let st = &mut *st5.borrow_mut();
                apply_op(st, &u5, op.clone());
            });
        }
        let st6 = Rc::clone(&state);
        let u6 = ui.clone();
        ui.save.connect_clicked(move |_| {
            let st = st6.borrow();
            if let Ok(path) = save_sidecar(&st) {
                u6.detail.set_text(&format!("saved {}", path.display()));
            }
        });

        // Undo/redo walk the recorded edit history; the selection follows the file the change
        // belongs to, so undoing a change made on another file still shows the right thumbnail.
        let st7 = Rc::clone(&state);
        let u7 = ui.clone();
        ui.undo.connect_clicked(move |_| {
            let st = &mut *st7.borrow_mut();
            if let Some(msg) = undo_last(st, &u7) {
                u7.detail.set_text(&msg);
            }
        });
        let st8 = Rc::clone(&state);
        let u8 = ui.clone();
        ui.redo.connect_clicked(move |_| {
            let st = &mut *st8.borrow_mut();
            if let Some(msg) = redo_next(st, &u8) {
                u8.detail.set_text(&msg);
            }
        });

        // Changing the sort key or the filter rebuilds the display order; the selection follows the
        // same file when it is still visible.
        let st_sort = Rc::clone(&state);
        let u_sort = ui.clone();
        ui.sort_box.connect_selected_notify(move |dd| {
            {
                let st = &mut *st_sort.borrow_mut();
                st.sort = sort_key_from_index(dd.selected());
            }
            apply_view(&st_sort, &u_sort);
        });

        let st_dir = Rc::clone(&state);
        let u_dir = ui.clone();
        ui.reverse.connect_clicked(move |_| {
            {
                let st = &mut *st_dir.borrow_mut();
                st.sort_dir = match st.sort_dir {
                    SortDirection::Natural => SortDirection::Reversed,
                    SortDirection::Reversed => SortDirection::Natural,
                };
            }
            apply_view(&st_dir, &u_dir);
        });

        let st_kind = Rc::clone(&state);
        let u_kind = ui.clone();
        ui.filter_kind.connect_selected_notify(move |dd| {
            {
                let st = &mut *st_kind.borrow_mut();
                st.filter.kind = filter_kind_from_index(dd.selected());
            }
            apply_view(&st_kind, &u_kind);
        });
        let st_hdr = Rc::clone(&state);
        let u_hdr = ui.clone();
        ui.filter_hdr_only.connect_toggled(move |b| {
            {
                let st = &mut *st_hdr.borrow_mut();
                st.filter.hdr_only = b.property::<bool>("active");
            }
            apply_view(&st_hdr, &u_hdr);
        });

        let st_transfer = Rc::clone(&state);
        let u_transfer = ui.clone();
        ui.transfer_box.connect_selected_notify(move |dd| {
            {
                let st = &mut *st_transfer.borrow_mut();
                st.filter.transfer = filter_transfer_from_index(dd.selected());
            }
            apply_view(&st_transfer, &u_transfer);
        });

        let sidebar = gtk::Box::new(gtk::Orientation::Vertical, 6);
        sidebar.set_size_request(240, -1);
        ui.results_scroll.set_child(Some(&ui.results));
        ui.results_scroll.set_min_content_height(120);
        ui.results_scroll.set_overlay_scrolling(false);
        ui.sub_scroll.set_child(Some(&ui.subfolders));
        ui.sub_scroll.set_min_content_height(120);
        ui.sub_scroll.set_overlay_scrolling(false);
        ui.timeline_scroll.set_child(Some(&ui.timeline));
        ui.timeline_scroll.set_min_content_height(100);
        ui.timeline_scroll.set_overlay_scrolling(false);
        ui.import_scroll.set_child(Some(&ui.import_list));
        ui.import_scroll.set_min_content_height(120);
        ui.import_scroll.set_overlay_scrolling(false);

        sidebar.append(&ui.search);
        sidebar.append(&ui.results_scroll);
        sidebar.append(&ui.sub_scroll);
        sidebar.append(&ui.timeline_scroll);
        sidebar.append(&ui.import_scroll);
        let import_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        import_row.set_homogeneous(true);
        import_row.append(&ui.import_expand);
        import_row.append(&ui.import_collapse);
        sidebar.append(&import_row);
        sidebar.append(&ui.sort_box);
        sidebar.append(&ui.reverse);
        ui.filter_hdr_only.set_label(Some("HDR only"));
        sidebar.append(&ui.filter_kind);
        sidebar.append(&ui.filter_hdr_only);
        sidebar.append(&ui.transfer_box);
        let list_scroll = gtk::ScrolledWindow::new();
        list_scroll.set_child(Some(&ui.list));
        list_scroll.set_vexpand(true);
        // Without a minimum the sidebar controls (2 dropdowns, 1 check button, 4 buttons, 2 labels)
        // squeeze the list to ~46 px, so only one row is ever visible.
        list_scroll.set_min_content_height(200);
        // overlay scrollbars fade in/out with animation, which makes the snapshot non-deterministic
        list_scroll.set_overlay_scrolling(false);
        sidebar.append(&list_scroll);
        sidebar.append(&ui.badge);
        sidebar.append(&ui.toggle);
        let edit_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        edit_row.set_homogeneous(true);
        let history_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        history_row.set_homogeneous(true);
        history_row.append(&ui.undo);
        history_row.append(&ui.redo);
        edit_row.append(&ui.rotate);
        edit_row.append(&ui.flip);
        edit_row.append(&ui.save);
        edit_row.append(&ui.delete);
        let clip_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        clip_row.set_homogeneous(true);
        clip_row.append(&ui.copy);
        clip_row.append(&ui.cut);
        clip_row.append(&ui.paste);
        sidebar.append(&clip_row);
        let rename_row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        // a non-editable entry draws no blinking caret, which keeps snapshots deterministic
        ui.rename_entry.set_editable(false);
        ui.rename_entry.set_hexpand(true);
        rename_row.append(&ui.rename_entry);
        rename_row.append(&ui.rename);
        sidebar.append(&rename_row);
        sidebar.append(&edit_row);
        sidebar.append(&history_row);
        sidebar.append(&ui.progress);
        sidebar.append(&ui.status);

        let grid_scroll = gtk::ScrolledWindow::new();
        grid_scroll.set_child(Some(&ui.grid));
        grid_scroll.set_vexpand(true);

        let center = gtk::Box::new(gtk::Orientation::Vertical, 6);
        center.set_hexpand(true);
        center.append(&grid_scroll);
        center.append(&ui.detail);
        center.append(&ui.info);
        center.append(&ui.picture);

        let content = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        content.append(&sidebar);
        content.append(&center);
        window.set_child(Some(&content));

        let st_sub = Rc::clone(&state);
        let u_sub = ui.clone();
        let paths = Rc::clone(&ui.subfolder_paths);
        // `row-activated`, not `row-selected`: appending rows to a Single-selection ListBox
        // auto-selects the first row, which would navigate into it during the initial build.
        let st_search = Rc::clone(&state);
        let u_search = ui.clone();
        ui.search.connect_changed(move |entry| {
            run_search(&st_search, &u_search, &entry.text());
        });
        let st_results = Rc::clone(&state);
        let u_results = ui.clone();
        ui.results.connect_row_activated(move |_list, row| {
            let item = u_results.result_rows.borrow().get(row.index() as usize).cloned();
            let Some((path, is_folder)) = item else { return };
            if is_folder {
                open_folder(&st_results, &u_results, &path);
            } else {
                open_and_select(&st_results, &u_results, &path);
            }
        });

        ui.subfolders.connect_row_activated(move |_list, row| {
            let path = paths.borrow().get(row.index() as usize).cloned();
            if let Some(p) = path {
                open_folder(&st_sub, &u_sub, &p);
            }
        });

        let st_tl = Rc::clone(&state);
        let u_tl = ui.clone();
        ui.timeline.connect_row_activated(move |_list, row| {
            let key = u_tl.timeline_months.borrow().get(row.index() as usize).cloned();
            if let Some(k) = key {
                select_month(&st_tl, &u_tl, &k);
            }
        });
        let st_imp = Rc::clone(&state);
        let u_imp = ui.clone();
        ui.import_list.connect_row_activated(move |_list, row| {
            let kind = u_imp.import_rows.borrow().get(row.index() as usize).cloned();
            match kind {
                Some(ImportRow::Header(g)) => {
                    u_imp.import_model.borrow_mut().toggle(g);
                    render_import_rows(&u_imp);
                }
                Some(ImportRow::Item(i)) => {
                    let path = u_imp.import_model.borrow().items.get(i).map(|x| x.path.clone());
                    let Some(p) = path else { return };
                    // Select the file in the main list when it is visible in the current view.
                    let pos = {
                        let st = st_imp.borrow();
                        st.display.iter().position(|&e| st.entries[e].path == p)
                    };
                    if let Some(pos) = pos {
                        record_click(&st_imp, &u_imp, pos, ClickMods::default());
                    }
                }
                None => {}
            }
        });

        let u_expand = ui.clone();
        ui.import_expand.connect_clicked(move |_| {
            u_expand.import_model.borrow_mut().set_all_expanded(true);
            render_import_rows(&u_expand);
        });
        let u_collapse = ui.clone();
        ui.import_collapse.connect_clicked(move |_| {
            u_collapse.import_model.borrow_mut().set_all_expanded(false);
            render_import_rows(&u_collapse);
        });

        rebuild_subfolders(&state, &ui);
        rebuild_timeline(&state, &ui);
        rescan_import(&state, &ui);

        let key = gtk::EventControllerKey::new();
        let st_key = Rc::clone(&state);
        let u_key = ui.clone();
        key.connect_key_pressed(move |_, keyval, _keycode, _mods| {
            let dir = match keyval {
                gdk::Key::Up => Direction::Up,
                gdk::Key::Down => Direction::Down,
                gdk::Key::Left => Direction::Left,
                gdk::Key::Right => Direction::Right,
                gdk::Key::Home => Direction::Home,
                gdk::Key::End => Direction::End,
                _ => return glib::Propagation::Proceed,
            };
            if move_selection(&st_key, &u_key, dir).is_some() {
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        let u_mods = ui.clone();
        key.connect_key_pressed(move |_, keyval, _keycode, mods| {
            let m = match keyval {
                gdk::Key::Control_L | gdk::Key::Control_R => Some(ClickMods {
                    ctrl: mods.contains(gdk::ModifierType::CONTROL_MASK),
                    shift: u_mods.mods.borrow().shift,
                }),
                gdk::Key::Shift_L | gdk::Key::Shift_R => Some(ClickMods {
                    ctrl: u_mods.mods.borrow().ctrl,
                    shift: mods.contains(gdk::ModifierType::SHIFT_MASK),
                }),
                _ => None,
            };
            if let Some(m) = m {
                *u_mods.mods.borrow_mut() = m;
            }
            glib::Propagation::Proceed
        });
        let u_mods2 = ui.clone();
        key.connect_key_released(move |_, keyval, _keycode, mods| {
            let m = match keyval {
                gdk::Key::Control_L | gdk::Key::Control_R => Some(ClickMods {
                    ctrl: mods.contains(gdk::ModifierType::CONTROL_MASK),
                    shift: u_mods2.mods.borrow().shift,
                }),
                gdk::Key::Shift_L | gdk::Key::Shift_R => Some(ClickMods {
                    ctrl: u_mods2.mods.borrow().ctrl,
                    shift: mods.contains(gdk::ModifierType::SHIFT_MASK),
                }),
                _ => None,
            };
            if let Some(m) = m {
                *u_mods2.mods.borrow_mut() = m;
            }
        });

        let st_del = Rc::clone(&state);
        let u_del = ui.clone();
        ui.delete.connect_clicked(move |_| {
            let paths: Vec<PathBuf> = {
                let st = st_del.borrow();
                u_del
                    .multi
                    .borrow()
                    .set
                    .iter()
                    .filter_map(|&i| st.entries.get(i).map(|e| e.path.clone()))
                    .collect()
            };
            if paths.is_empty() {
                return;
            }
            let folder = st_del.borrow().folder.clone();
            u_del.bar.borrow_mut().begin_total("deleting", paths.len());
            for (i, path) in paths.iter().enumerate() {
                u_del.bar.borrow_mut().update(i + 1, &format!("deleting {}", path.display()));
            }
            let report = delete_paths(&paths, &trash_root(), &iso8601_utc(std::time::SystemTime::now()));
            u_del.bar.borrow_mut().finish(&format!("deleted {}", paths.len()), *u_del.clock.borrow());
            // `open_folder` returns early when the folder is unchanged, so rescan here and drop the
            // stale indices before rebuilding the view.
            let entries = scan_folder(&folder);
            {
                let mut st = st_del.borrow_mut();
                st.entries = entries;
                st.selected = None;
            }
            *u_del.multi.borrow_mut() = Selection::default();
            u_del.cache.borrow_mut().invalidate(&folder);
            apply_view(&st_del, &u_del);
            rebuild_subfolders(&st_del, &u_del);
            let trashed = report.iter().filter(|(_, o)| matches!(o, DeleteOutcome::Trashed(_))).count();
            let removed = report.iter().filter(|(_, o)| matches!(o, DeleteOutcome::Removed)).count();
            let failed = report.iter().filter(|(_, o)| matches!(o, DeleteOutcome::Failed(_))).count();
            u_del.status.set_text(&format!("deleted: {trashed} to trash, {removed} permanent, {failed} failed"));
        });

        let st_copy = Rc::clone(&state);
        let u_copy = ui.clone();
        ui.copy.connect_clicked(move |_| set_clipboard_from_selection(&st_copy, &u_copy, ClipboardMode::Copy));
        let st_cut = Rc::clone(&state);
        let u_cut = ui.clone();
        ui.cut.connect_clicked(move |_| set_clipboard_from_selection(&st_cut, &u_cut, ClipboardMode::Cut));

        let st_paste = Rc::clone(&state);
        let u_paste = ui.clone();
        ui.paste.connect_clicked(move |_| {
            let clip = u_paste.clipboard.borrow().clone();
            if clip.paths.is_empty() {
                return;
            }
            let folder = st_paste.borrow().folder.clone();
            u_paste.bar.borrow_mut().begin_total("pasting", clip.paths.len());
            for (i, path) in clip.paths.iter().enumerate() {
                u_paste.bar.borrow_mut().update(i + 1, &format!("pasting {}", path.display()));
            }
            let report = paste(&clip, &folder);
            let ok_before = report.iter().filter(|r| r.error.is_none()).count();
            u_paste.bar.borrow_mut().finish(&format!("pasted {}", ok_before), *u_paste.clock.borrow());
            let entries = scan_folder(&folder);
            {
                let mut st = st_paste.borrow_mut();
                st.entries = entries;
                st.selected = None;
            }
            *u_paste.multi.borrow_mut() = Selection::default();
            u_paste.cache.borrow_mut().invalidate(&folder);
            apply_view(&st_paste, &u_paste);
            rebuild_subfolders(&st_paste, &u_paste);
            let ok = report.iter().filter(|r| r.error.is_none()).count();
            let failed = report.len() - ok;
            let verb = match clip.mode {
                ClipboardMode::Copy => "copied",
                ClipboardMode::Cut => "moved",
            };
            u_paste.status.set_text(&format!("pasted {verb}: {ok} ok, {failed} failed"));
        });

        let st_rename = Rc::clone(&state);
        let u_rename = ui.clone();
        ui.rename.connect_clicked(move |_| {
            let folder = st_rename.borrow().folder.clone();
            let name = u_rename.rename_entry.text().to_string();
            // The parent's cached entry still names the old child, so drop it before navigating.
            if let Some(parent) = folder.parent() {
                u_rename.cache.borrow_mut().invalidate(parent);
            }
            match rename_dir(&folder, &name) {
                Ok(target) => open_folder(&st_rename, &u_rename, &target),
                Err(e) => u_rename.status.set_text(&format!("rename failed: {e}")),
            }
        });

        window.add_controller(key);

        // The initial selection is row 0, but `set_selected` would not fire a notify for it, so
        // record it explicitly to keep the multi-select model consistent with what is displayed.
        if !state.borrow().entries.is_empty() {
            let start = state.borrow().selected.unwrap_or(0);
            record_click(&state, &ui, start, ClickMods::default());
        }

        refresh(&state.borrow(), &ui);
        App { window, state, ui }
    }
}

/// Record a selection change in the multi-select model. Programmatic `set_selected` calls go
/// through here too, so the model matches what the user sees even when the notify signal does not
/// fire because the value was already that row.
pub fn set_clipboard_from_selection(state: &Rc<RefCell<State>>, ui: &Ui, mode: ClipboardMode) {
    let paths: Vec<PathBuf> = {
        let st = state.borrow();
        ui.multi.borrow().set.iter().filter_map(|&i| st.entries.get(i).map(|e| e.path.clone())).collect()
    };
    *ui.clipboard.borrow_mut() = set_clipboard(&paths, mode);
}

pub fn record_click(state: &Rc<RefCell<State>>, ui: &Ui, idx: usize, mods: ClickMods) {
    let m = apply_click(&ui.multi.borrow().clone(), idx, mods);
    *ui.multi.borrow_mut() = m.clone();
    mirror_model(ui, &m);
    state.borrow_mut().selected = Some(idx);
    let st = state.borrow();
    refresh(&st, ui);
}

/// Push the core selection set into the GTK selection model so the visible highlight matches the
/// model. Without this the highlight is still single-row even though the core model holds a set.
pub fn mirror_model(ui: &Ui, sel: &Selection) {
    let n = *ui.item_count.borrow();
    *ui.syncing.borrow_mut() = true;
    // The grid is driven by the MultiSelection model; the ListBox has its own internal model
    // (`set_selection_model` is not bound in gtk4-rs), so mirror through the rows instead.
    for i in 0..n {
        if sel.contains(i) {
            ui.selection.select_item(i as u32, false);
        } else {
            ui.selection.unselect_item(i as u32);
        }
    }
    ui.list.unselect_all();
    for i in 0..n {
        if sel.contains(i) {
            if let Some(row) = ui.list.row_at_index(i as i32) {
                ui.list.select_row(Some(&row));
            }
        }
    }
    *ui.syncing.borrow_mut() = false;
}

pub fn refresh(state: &State, ui: &Ui) {
    let mut text = format!(
        "{} of {} · {} · sorted by {} ({})",
        state.display.len(),
        state.entries.len(),
        state.filter.label(),
        state.sort.label(),
        state.sort_dir.label()
    );
    if ui.multi.borrow().count() > 1 {
        text.push_str(&format!(" · selected {}", ui.multi.borrow().count()));
    }
    let h = ui.history.borrow();
    if h.undo_len() + h.redo_len() > 0 {
        text.push_str(&format!(" · history {} undo, {} redo", h.undo_len(), h.redo_len()));
    }
    ui.status.set_text(&text);

    // The progress row is only shown when a task or a completion message is present.
    let mut bar = ui.bar.borrow_mut();
    bar.tick(*ui.clock.borrow());
    ui.progress.set_visible(bar.is_visible());
    ui.progress.set_text(&bar.text());

    // Re-label rows and tiles: the transfer-function badge must follow the HDR/SDR toggle.
    for (path, label) in ui.labels.borrow().iter() {
        let e = match state.entries.iter().find(|e| &e.path == path) {
            Some(e) => e,
            None => continue,
        };
        label.set_text(&format!(
            "{}  ·  {}",
            e.name(),
            badge_label(e.signal.as_ref(), state.hdr_enabled)
        ));
    }

    // Re-paint thumbnails so list and grid show the applied edit (crop/rotate/flip).
    let pics = ui.thumbs_pics.borrow().clone();
    for (path, pic) in pics.iter() {
        paint_thumbnail(ui, state, path, pic);
    }

    match state.selected {
        Some(i) => {
            let e = match entry_at(state, i) {
                Some(e) => e,
                None => return,
            };
            let edit = effective_edit(state, e);
            let from_sidecar = state.ops.is_empty() && e.edit.is_some();
            let size = match decode_frame(&e.path) {
                Ok(frame) => {
                    match apply_edit(&frame, &edit) {
                        Ok(tex) => {
                            ui.picture.set_paintable(Some(&tex));
                            (tex.width() as u32, tex.height() as u32)
                        }
                        Err(_) => {
                            ui.picture.set_paintable(None::<&gdk::Paintable>);
                            edit.display_size(frame.width(), frame.height())
                        }
                    }
                }
                Err(_) => {
                    ui.picture.set_paintable(None::<&gdk::Paintable>);
                    (0, 0)
                }
            };
            if edit.is_identity() {
                ui.detail.set_text(&format!("{}  ·  {}", e.name(), e.kind.label()));
            } else {
                let suffix = if from_sidecar { " (sidecar)" } else { "" };
                ui.detail.set_text(&format!(
                    "{}  ·  {}  ·  {}x{}{}",
                    e.name(),
                    e.kind.label(),
                    size.0,
                    size.1,
                    suffix
                ));
            }
            let desc = e.signal.as_ref().map(|s| s.describe()).unwrap_or_default();
            let exif = e.signal
                .as_ref()
                .and_then(|s| s.exif.as_ref())
                .and_then(|x| x.summary())
                .unwrap_or_default();
            let video = e.video.as_ref().map(|v| v.describe()).unwrap_or_default();
            let parts: Vec<String> = [desc, video, exif].into_iter().filter(|s| !s.is_empty()).collect();
            ui.info.set_text(&parts.join(" · "));
            ui.badge.set_text(badge_label(e.signal.as_ref(), state.hdr_enabled));
        }
        None => {
            ui.detail.set_text("(no selection)");
            ui.info.set_text("");
            ui.badge.set_text("SDR");
            ui.picture.set_paintable(None::<&gdk::Paintable>);
        }
    }
}
