//! Keyboard selection (`select.move` / `select.clear`): moves run over the *visible* list, so a
//! collapsed month or a filter is skipped, and an edge move leaves the selection where it was.

use std::path::PathBuf;

use serde_json::json;
use spectrum_core::command::{dispatch, CommandError};
use spectrum_core::model::{Library, ViewMode};
use spectrum_core::sort::SortKey;
use spectrum_testkit::fixture;

fn library(tag: &str, files: &[&str]) -> Library {
    let dir = std::env::temp_dir().join(format!("spectrum-nav-{tag}"));
    std::fs::create_dir_all(&dir).unwrap();
    for name in files {
        fixture::write_png_fixture(&dir, name, 40, 40, [10, 20, 30]);
    }
    let mut lib = Library::new(&dir);
    lib.sort = SortKey::Name;
    lib.view = ViewMode::Months;
    lib.rescan();
    lib
}

fn move_sel(lib: &mut Library, direction: &str, columns: usize) -> serde_json::Value {
    dispatch(
        lib,
        "select.move",
        &json!({"direction": direction, "columns": columns}),
    )
    .unwrap_or_else(|_| panic!("select.move {direction} should succeed"))
}

fn names(lib: &Library) -> Vec<String> {
    lib.visible()
        .iter()
        .map(|&i| lib.entries[i].name())
        .collect()
}

#[test]
fn arrows_step_through_the_visible_list_in_column_order() {
    let mut lib = library(
        "step",
        &[
            "a.png", "b.png", "c.png", "d.png", "e.png", "f.png", "g.png",
        ],
    );
    assert_eq!(
        names(&lib),
        vec!["a.png", "b.png", "c.png", "d.png", "e.png", "f.png", "g.png"]
    );

    // No selection yet: the first arrow lands on the first item, wherever it points.
    let answer = move_sel(&mut lib, "right", 3);
    assert_eq!(answer["moved"], json!(true));
    assert_eq!(
        lib.entries[lib.sole().unwrap_or(usize::MAX)].name(),
        "a.png"
    );

    move_sel(&mut lib, "right", 3);
    assert_eq!(
        lib.entries[lib.sole().unwrap_or(usize::MAX)].name(),
        "b.png"
    );
    // Down is one row, not one item.
    move_sel(&mut lib, "down", 3);
    assert_eq!(
        lib.entries[lib.sole().unwrap_or(usize::MAX)].name(),
        "e.png"
    );
    // Up mirrors it.
    move_sel(&mut lib, "up", 3);
    assert_eq!(
        lib.entries[lib.sole().unwrap_or(usize::MAX)].name(),
        "b.png"
    );
    // Left steps back.
    move_sel(&mut lib, "left", 3);
    assert_eq!(
        lib.entries[lib.sole().unwrap_or(usize::MAX)].name(),
        "a.png"
    );
}

#[test]
fn home_and_end_jump_to_the_ends() {
    let mut lib = library("ends", &["a.png", "b.png", "c.png"]);
    // From nothing, `End` picks the last item.
    move_sel(&mut lib, "end", 1);
    assert_eq!(
        lib.entries[lib.sole().unwrap_or(usize::MAX)].name(),
        "c.png"
    );
    move_sel(&mut lib, "home", 1);
    assert_eq!(
        lib.entries[lib.sole().unwrap_or(usize::MAX)].name(),
        "a.png"
    );
}

#[test]
fn moving_past_an_edge_changes_nothing_and_reports_it() {
    let mut lib = library("edge", &["a.png", "b.png"]);
    move_sel(&mut lib, "home", 1);
    let before = lib.selection.clone();

    let answer = move_sel(&mut lib, "left", 1);
    assert_eq!(answer["moved"], json!(false), "already at the first item");
    assert_eq!(answer["position"], json!(0));
    assert_eq!(lib.selection, before, "the model is untouched");

    move_sel(&mut lib, "end", 1);
    let at_end = lib.selection.clone();
    let answer = move_sel(&mut lib, "down", 1);
    assert_eq!(answer["moved"], json!(false));
    assert_eq!(lib.selection, at_end);
}

#[test]
fn a_collapsed_group_is_stepped_over() {
    let dir = std::env::temp_dir().join("spectrum-nav-collapse");
    std::fs::create_dir_all(&dir).unwrap();
    // Two months: the older one gets collapsed, so Right from the newest month must wrap past it.
    fixture::write_png_fixture(&dir, "aug-a.png", 40, 40, [10, 20, 30]);
    fixture::write_png_fixture(&dir, "aug-b.png", 40, 40, [10, 20, 30]);
    let mut lib = Library::new(&dir);
    lib.sort = SortKey::Name;
    lib.view = ViewMode::All;
    lib.rescan();
    let groups = lib.groups();
    assert_eq!(groups.len(), 1, "the fixtures share one month");

    lib.collapsed.push(groups[0].key.clone());
    let err = dispatch(
        &mut lib,
        "select.move",
        &json!({"direction": "right", "columns": 3}),
    )
    .expect_err("nothing visible means nothing to select");
    assert!(matches!(err, CommandError::Disabled(_)), "got {err:?}");
    assert!(
        lib.selection.set.is_empty(),
        "a disabled move selects nothing"
    );

    lib.collapsed.clear();
    move_sel(&mut lib, "right", 3);
    assert!(
        lib.selection.count() == 1,
        "with the group open it works again"
    );
}

#[test]
fn a_filter_is_respected_when_moving() {
    let dir = std::env::temp_dir().join("spectrum-nav-filter");
    std::fs::create_dir_all(&dir).unwrap();
    fixture::write_png_fixture(&dir, "still.png", 40, 40, [10, 20, 30]);
    fixture::write_png_fixture(&dir, "movie.mp4", 40, 40, [10, 20, 30]);
    let mut lib = Library::new(&dir);
    lib.sort = SortKey::Name;
    lib.rescan();
    dispatch(&mut lib, "filter.kind", &json!({"kind": "photo"})).expect("filter to photos only");
    let visible = lib.visible();
    assert_eq!(visible.len(), 1, "the video is filtered out");
    move_sel(&mut lib, "end", 1);
    assert_eq!(
        lib.entries[lib.sole().unwrap_or(usize::MAX)].name(),
        "still.png"
    );
    let answer = move_sel(&mut lib, "right", 1);
    assert_eq!(
        answer["moved"],
        json!(false),
        "there is nothing past the only photo"
    );
}

#[test]
fn a_single_row_grid_moves_one_item_per_key() {
    let mut lib = library("rows", &["a.png", "b.png", "c.png"]);
    move_sel(&mut lib, "right", 1);
    assert_eq!(
        lib.entries[lib.sole().unwrap_or(usize::MAX)].name(),
        "a.png"
    );
    move_sel(&mut lib, "right", 1);
    assert_eq!(
        lib.entries[lib.sole().unwrap_or(usize::MAX)].name(),
        "b.png"
    );
    move_sel(&mut lib, "down", 1);
    assert_eq!(
        lib.entries[lib.sole().unwrap_or(usize::MAX)].name(),
        "c.png"
    );
}

#[test]
fn clear_removes_the_selection_and_reports_how_many() {
    let mut lib = library("clear", &["a.png", "b.png"]);
    move_sel(&mut lib, "home", 1);
    let all = lib.visible();
    lib.select(&all);
    let answer = dispatch(&mut lib, "select.clear", &json!({})).expect("clear");
    assert_eq!(answer["cleared"], json!(2));
    assert!(lib.selection.set.is_empty());
    assert_eq!(lib.selection.primary, None);
}

#[test]
fn an_unknown_direction_is_a_parameter_error() {
    let mut lib = library("bad", &["a.png"]);
    let err = dispatch(
        &mut lib,
        "select.move",
        &json!({"direction": "sideways", "columns": 1}),
    )
    .expect_err("unknown direction");
    assert!(matches!(err, CommandError::BadParam(_)), "got {err:?}");
    assert!(lib.selection.set.is_empty());
    let err = dispatch(&mut lib, "select.move", &json!({"direction": "left"}))
        .expect_err("columns is required");
    assert!(matches!(err, CommandError::BadParam(_)), "got {err:?}");
}

#[test]
fn moving_never_touches_the_files() {
    let dir = std::env::temp_dir().join("spectrum-nav-files");
    std::fs::create_dir_all(&dir).unwrap();
    let path = fixture::write_png_fixture(&dir, "a.png", 40, 40, [10, 20, 30]);
    let before = std::fs::read(&path).unwrap_or_default();
    let mut lib = Library::new(&dir);
    lib.rescan();
    for direction in ["right", "down", "end", "home", "up"] {
        dispatch(
            &mut lib,
            "select.move",
            &json!({"direction": direction, "columns": 2}),
        )
        .ok();
    }
    assert_eq!(std::fs::read(&path).unwrap_or_default(), before);
    assert_eq!(sidecars(&dir), vec![] as Vec<PathBuf>);
}

fn sidecars(dir: &PathBuf) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .unwrap_or_else(|_| panic!("dir unreadable"))
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("xmp"))
        .collect()
}
