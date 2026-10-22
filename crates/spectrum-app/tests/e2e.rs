use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, UNIX_EPOCH};

use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use spectrum_app::app::AppState;
use spectrum_core::hdr::DisplayMode;
use spectrum_core::model::{InspectorTab, ViewMode};
use spectrum_core::scan::MediaKind;
use spectrum_core::sort::{SortDirection, SortKey};
use spectrum_testkit::fixture;

// ISO 14496-12 codes.
const BT2020: u16 = 9;
const BT709: u16 = 1;
const SRGB: u16 = 13;
const HLG: u16 = 18;

/// Fixed timestamps (UTC) so the month groups — and with them the snapshots — do not drift with
/// the date the tests happen to run on.
const AUG_2016: u64 = 1_470_391_200; // 2016-08-05 10:00
const SEP_2016: u64 = 1_472_841_000; // 2016-09-02 18:30

/// One directory per test: the tests run in parallel, and a fixture rewritten while another test is
/// scanning it would change what that test sees. The names are fixed so the sidebar row text inside
/// a snapshot stays reproducible.
fn fixture_dir(tag: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("spectrum-egui-{tag}"));
    std::fs::create_dir_all(&dir).unwrap_or_default();
    let hlg = fixture::write_fixture(&dir, "hlg_photo.heic", BT2020, HLG, 0, true);
    let sdr = fixture::write_fixture(&dir, "sdr_photo.heic", BT709, SRGB, 0, false);
    stamp(&hlg, AUG_2016);
    stamp(&sdr, SEP_2016);
    dir
}

fn stamp(path: &Path, secs: u64) {
    if let Ok(file) = std::fs::OpenOptions::new().write(true).open(path) {
        let _ = file.set_modified(UNIX_EPOCH + Duration::from_secs(secs));
    }
}

/// Which entry is which, so the assertions do not depend on the sort order.
fn fixture_indices(app: &AppState) -> (usize, usize) {
    let index = |name: &str| {
        app.lib
            .entries
            .iter()
            .position(|e| e.name() == name)
            .unwrap()
    };
    (index("hlg_photo.heic"), index("sdr_photo.heic"))
}

fn harness(state: Rc<RefCell<AppState>>) -> Harness<'static> {
    Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .with_theme(egui::Theme::Light)
        .build_ui(move |ui| state.borrow_mut().render(ui))
}

#[test]
fn the_grid_lists_every_fixture_by_name() {
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir("grid"))));
    let mut harness = harness(state.clone());
    harness.run();

    harness.get_by_label("hlg_photo.heic");
    harness.get_by_label("sdr_photo.heic");
    assert_eq!(
        state.borrow().lib.selection.count(),
        0,
        "drawing selects nothing"
    );
    assert_eq!(
        state.borrow().lib.folders.len(),
        1,
        "the screen starts with one folder"
    );
}

#[test]
fn selecting_a_tile_marks_it_and_shows_the_badge() {
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir("badge"))));
    let mut harness = harness(state.clone());
    harness.run();

    harness.get_by_label("hlg_photo.heic").click();
    harness.run();
    assert_eq!(state.borrow().lib.selection.count(), 1);
    assert_eq!(
        harness.query_all_by_label("HDR").count(),
        1,
        "the HLG tile reads HDR while HDR output is on"
    );

    harness.get_by_label("HDR / SDR toggle").click();
    harness.run();
    assert!(
        !state.borrow().lib.hdr_enabled,
        "the toggle flipped the display mode"
    );
    assert_eq!(
        harness.query_all_by_label("HDR").count(),
        0,
        "nothing reads HDR now"
    );
    assert!(
        harness.query_all_by_label("SDR").count() >= 2,
        "both tiles read SDR"
    );

    harness.get_by_label("HDR / SDR toggle").click();
    harness.run();
    harness.get_by_label("sdr_photo.heic").click();
    harness.run();
    let (hlg, sdr) = fixture_indices(&state.borrow());
    assert_eq!(
        state.borrow().lib.badge_for(sdr),
        "SDR",
        "an sRGB file stays SDR"
    );
    assert_eq!(
        state.borrow().lib.badge_for(hlg),
        "HLG",
        "the HLG file reports its transfer"
    );
    assert_eq!(state.borrow().lib.mode_for(sdr), DisplayMode::Sdr);
    assert_eq!(state.borrow().lib.mode_for(hlg), DisplayMode::Hdr);
}

#[test]
fn the_sidebar_holds_only_folders_and_adding_one_opens_it() {
    let first = fixture_dir("folders-a");
    let second = fixture_dir("folders-b");
    let state = Rc::new(RefCell::new(AppState::new(first.clone())));
    let mut harness = harness(state.clone());
    harness.run();

    assert_eq!(harness.query_all_by_label_contains("Library").count(), 0);
    assert_eq!(harness.query_all_by_label_contains("Filter").count(), 0);
    harness.get_by_label("spectrum-egui-folders-a  2").click();
    harness.run();
    assert_eq!(
        state.borrow().lib.active,
        0,
        "clicking the open folder is a no-op"
    );

    let field = harness.get_by_role(Role::TextInput);
    field.focus();
    field.type_text(&second.display().to_string());
    harness.run();
    harness.get_by_label("Add folder").click();
    harness.run();

    assert_eq!(
        state.borrow().lib.folders.len(),
        2,
        "the folder joined the list"
    );
    assert_eq!(state.borrow().lib.active, 1, "and became the open one");
    assert_eq!(state.borrow().lib.folder(), second.as_path());
    assert_eq!(state.borrow().lib.entries.len(), 2);
    assert!(
        state.borrow().add_path.is_empty(),
        "the field clears on success"
    );
    harness.get_by_label("spectrum-egui-folders-b  2");
}

#[test]
fn enter_in_the_add_folder_field_submits_it() {
    let first = fixture_dir("enter-a");
    let second = fixture_dir("enter-b");
    let state = Rc::new(RefCell::new(AppState::new(first)));
    let mut harness = harness(state.clone());
    harness.run();

    let field = harness.get_by_role(Role::TextInput);
    field.focus();
    field.type_text(&second.display().to_string());
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();

    assert_eq!(
        state.borrow().lib.folders.len(),
        2,
        "Enter submitted the path"
    );
    assert_eq!(state.borrow().lib.active, 1);
    assert!(state.borrow().add_path.is_empty(), "the field cleared");
}

#[test]
fn a_folder_path_that_is_not_a_directory_is_rejected_and_stays_in_the_field() {
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir("badpath"))));
    let mut harness = harness(state.clone());
    harness.run();

    let field = harness.get_by_role(Role::TextInput);
    field.focus();
    field.type_text("/nonexistent/library-here");
    harness.run();
    harness.get_by_label("Add folder").click();
    harness.run();

    assert_eq!(state.borrow().lib.folders.len(), 1, "nothing was added");
    assert_eq!(state.borrow().lib.active, 0);
    assert_eq!(
        state.borrow().add_path,
        "/nonexistent/library-here",
        "a rejected path stays in the field so the user can fix it"
    );
    assert!(
        state.borrow().notice.is_some(),
        "the reason is in the status bar"
    );
}

#[test]
fn removing_a_folder_from_the_sidebar_leaves_the_files_on_disk() {
    let dir = fixture_dir("remove");
    let files = [dir.join("hlg_photo.heic"), dir.join("sdr_photo.heic")];
    let state = Rc::new(RefCell::new(AppState::new(dir.clone())));
    let mut harness = harness(state.clone());
    harness.run();

    harness.get_by_label("Remove folder").click();
    harness.run();
    assert_eq!(state.borrow().lib.folders.len(), 0);
    assert!(
        state.borrow().lib.entries.is_empty(),
        "no folder means no tiles"
    );
    for file in &files {
        assert!(file.exists(), "{} still on disk", file.display());
    }
    assert_eq!(
        harness
            .query_all_by_label_contains("spectrum-egui-remove")
            .count(),
        0,
        "the row is gone from the sidebar"
    );
}

#[test]
fn the_titlebar_and_toolbar_drive_view_filter_and_sort() {
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir("toolbar"))));
    let mut harness = harness(state.clone());
    harness.run();

    harness.get_by_label("Months").click();
    harness.run();
    assert_eq!(
        state.borrow().lib.view,
        ViewMode::Months,
        "view.set dispatched"
    );

    harness.get_by_label("August 2016  ·  1 Photos").click();
    harness.run();
    assert_eq!(state.borrow().lib.filter.month.as_deref(), Some("2016-08"));

    harness.get_by_label("Videos").click();
    harness.run();
    assert_eq!(state.borrow().lib.filter.kind, Some(MediaKind::Video));

    harness.get_by_label("Clear filter").click();
    harness.run();
    assert_eq!(state.borrow().lib.filter.kind, None);
    assert_eq!(
        state.borrow().lib.filter.month,
        None,
        "Clear filter clears the month too"
    );

    harness.get_by_label("HDR only").click();
    harness.run();
    assert!(state.borrow().lib.filter.hdr_only, "filter.hdr dispatched");
    assert_eq!(
        state.borrow().lib.visible().len(),
        1,
        "only the HLG file survives"
    );

    harness.get_by_label("Size").click();
    harness.run();
    assert_eq!(state.borrow().lib.sort, SortKey::Size);
    harness.get_by_label("Natural order").click();
    harness.run();
    assert_eq!(
        state.borrow().lib.direction,
        SortDirection::Reversed,
        "the button flips direction"
    );
}

#[test]
fn the_inspector_edits_the_selected_file() {
    let dir = fixture_dir("edit");
    let state = Rc::new(RefCell::new(AppState::new(dir.clone())));
    let mut harness = harness(state.clone());
    harness.run();

    harness.get_by_label("hlg_photo.heic").click();
    harness.run();
    let original = std::fs::read(dir.join("hlg_photo.heic")).unwrap();

    harness.get_by_label("Rotate ⟳").click();
    harness.run();
    harness.get_by_label("Flip horizontal").click();
    harness.run();
    assert_eq!(
        std::fs::read(dir.join("hlg_photo.heic")).unwrap(),
        original,
        "editing writes a sidecar, never the original file"
    );
    let sidecar = dir.join("hlg_photo.heic.xmp");
    assert!(sidecar.exists(), "the edit lives beside the file");

    harness.get_by_label("Info").click();
    harness.run();
    assert_eq!(
        state.borrow().lib.tab,
        InspectorTab::Info,
        "inspector.tab dispatched"
    );
    assert_eq!(
        harness.query_all_by_label("Rotate ⟳").count(),
        0,
        "the Adjust controls are hidden while Info is open"
    );

    harness.get_by_label("Adjust").click();
    harness.run();
    harness.get_by_label("Restore").click();
    harness.run();
    assert!(!sidecar.exists(), "Restore removes the sidecar");
    assert_eq!(std::fs::read(dir.join("hlg_photo.heic")).unwrap(), original);
}

#[test]
fn a_rejected_command_is_reported_and_changes_nothing() {
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir("reject"))));
    let mut harness = harness(state.clone());
    harness.run();

    harness.get_by_label("Months").click();
    harness.run();
    harness.get_by_label("August 2016  ·  1 Photos").click();
    harness.run();
    let month_before = state.borrow().lib.filter.month.clone();
    let view_before = state.borrow().lib.view;

    let accepted = state
        .borrow_mut()
        .run("month.open", serde_json::json!({"key": "1999-1"}));
    assert!(!accepted, "a month key that is not YYYY-MM is rejected");
    assert_eq!(state.borrow().lib.filter.month, month_before);
    assert_eq!(
        state.borrow().lib.view,
        view_before,
        "a rejection changes nothing"
    );
    assert!(
        state.borrow().notice.is_some(),
        "the reason is available for the status bar"
    );

    state
        .borrow_mut()
        .run("filter.clear", serde_json::json!({}));
    assert!(
        state.borrow().notice.is_none(),
        "the next success clears the notice"
    );
}

/// The overflow bug this screen had: a control laid out past the panel it belongs to keeps its
/// node in the tree but sits where another panel is, so a click there hits the wrong widget.
#[test]
fn no_control_sits_outside_the_window() {
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir("bounds"))));
    let mut harness = harness(state.clone());
    harness.run();

    let window = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0));
    let mut outside = Vec::new();
    for role in [Role::Button, Role::TextInput] {
        for node in harness.query_all_by_role(role) {
            if !window.contains(node.rect().center()) {
                outside.push(format!("{node:?}"));
            }
        }
    }
    assert!(
        outside.is_empty(),
        "controls outside the window: {outside:?}"
    );
}

#[test]
fn snapshot_library_grid() {
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir("snap-library"))));
    let mut harness = harness(state.clone());
    harness.run();
    harness.snapshot("library_all");
}

#[test]
fn snapshot_detail_adjust() {
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir("snap-detail"))));
    let mut harness = harness(state.clone());
    harness.run();
    harness.get_by_label("hlg_photo.heic").click();
    harness.run();
    harness.snapshot("detail_adjust");
}
