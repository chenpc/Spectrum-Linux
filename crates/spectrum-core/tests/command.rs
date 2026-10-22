use std::path::PathBuf;

use serde_json::json;
use spectrum_core::command::{describe, dispatch, lookup, specs, CommandError};
use spectrum_core::{Library, ViewMode};
use spectrum_testkit::fixture;

// ISO 14496-12 codes.
const BT2020: u16 = 9;
const BT709: u16 = 1;
const SRGB: u16 = 13;
const HLG: u16 = 18;

fn temp_dir(tag: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("spectrum-cmd-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    for entry in std::fs::read_dir(&dir).expect("list temp dir") {
        if let Ok(path) = entry.map(|e| e.path()) {
            let _ = std::fs::remove_file(path);
        }
    }
    dir
}

fn library_with(tag: &str) -> Library {
    let dir = temp_dir(tag);
    fixture::write_fixture(&dir, "hlg.heic", BT2020, HLG, 0, true);
    fixture::write_fixture(&dir, "sdr.heic", BT709, SRGB, 0, false);
    let mut lib = Library::new(&dir);
    lib.trash_root = dir.join("trash");
    lib
}

/// mtime drives month grouping; fixtures are written now, so pin them to distinct months.
fn set_mtime(path: &PathBuf, t: std::time::SystemTime) {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("open to set mtime");
    file.set_modified(t).expect("set mtime");
}

fn run(
    lib: &mut Library,
    id: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, CommandError> {
    dispatch(lib, id, &args)
}

/// The registry itself must be usable by automation: unique ids, nothing blank.
#[test]
fn registry_has_unique_ids_and_complete_metadata() {
    let list = specs();
    assert!(
        list.len() >= 15,
        "expected a real command set, got {}",
        list.len()
    );
    let mut ids: Vec<&str> = list.iter().map(|s| s.id).collect();
    let count = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), count, "duplicate command ids");
    for spec in list {
        assert!(!spec.label.is_empty(), "{} has no label", spec.id);
        assert!(
            !spec.params.is_empty(),
            "{} has no params description",
            spec.id
        );
        assert!(!spec.id.contains(' '), "{} has a space in its id", spec.id);
        assert!(lookup(spec.id).is_some(), "{} is not findable", spec.id);
    }
}

#[test]
fn describe_lists_every_command() {
    let value = describe();
    let array = value.as_array().expect("describe returns an array");
    assert_eq!(array.len(), specs().len());
    for entry in array {
        assert!(entry["id"].is_string(), "{entry}");
        assert!(entry["group"].is_string(), "{entry}");
        assert!(entry["params"].is_string(), "{entry}");
    }
}

#[test]
fn unknown_id_is_reported_with_the_id() {
    let mut lib = library_with("unknown");
    let err = run(&mut lib, "nope.nothing", json!({})).expect_err("unknown id must fail");
    assert!(matches!(err, CommandError::Unknown(_)), "{err:?}");
    assert!(err.to_string().contains("nope.nothing"), "{err}");
}

#[test]
fn view_set_changes_the_mode_and_rejects_a_bad_name() {
    let mut lib = library_with("view");
    assert_eq!(
        run(&mut lib, "view.set", json!({"view": "months"})).expect("ok")["view"],
        "Months"
    );
    assert_eq!(lib.view, ViewMode::Months);
    let err = run(&mut lib, "view.set", json!({"view": "weeks"})).expect_err("weeks is not a view");
    assert!(matches!(err, CommandError::BadParam(_)), "{err:?}");
    assert_eq!(
        lib.view,
        ViewMode::Months,
        "a rejected command must not change the model"
    );
}

#[test]
fn hdr_toggle_moves_the_badge_of_the_hlg_file() {
    let mut lib = library_with("hdr");
    let hlg = lib
        .entries
        .iter()
        .position(|e| e.name() == "hlg.heic")
        .expect("hlg fixture scanned");
    assert_eq!(
        lib.badge_for(hlg),
        "HLG",
        "HLG signalling reads as HLG while HDR is on"
    );
    assert_eq!(
        run(&mut lib, "hdr.toggle", json!({})).expect("ok")["hdr"],
        false
    );
    assert_eq!(
        lib.badge_for(hlg),
        "SDR",
        "turning HDR off tone-maps the badge"
    );
    let sdr = lib
        .entries
        .iter()
        .position(|e| e.name() == "sdr.heic")
        .expect("sdr fixture");
    assert_eq!(lib.badge_for(sdr), "SDR");
}

#[test]
fn selection_commands_use_the_same_semantics_as_clicks() {
    let mut lib = library_with("select");
    assert_eq!(
        run(&mut lib, "library.select", json!({"indices": [1, 0, 9]})).expect("ok")["count"],
        2,
        "out-of-range index is dropped"
    );
    assert_eq!(lib.selection.count(), 2);

    run(&mut lib, "library.clear", json!({})).expect("clear");
    assert_eq!(lib.selection.count(), 0);

    run(&mut lib, "grid.click", json!({"index": 0})).expect("plain click");
    assert_eq!(lib.selection.count(), 1);
    run(&mut lib, "grid.click", json!({"index": 1, "ctrl": true})).expect("ctrl click adds");
    assert_eq!(lib.selection.count(), 2);
    run(&mut lib, "grid.click", json!({"index": 0, "ctrl": true})).expect("ctrl click toggles off");
    assert_eq!(lib.selection.count(), 1);

    let err = run(&mut lib, "grid.click", json!({"index": 99})).expect_err("past the end");
    assert!(matches!(err, CommandError::BadParam(_)), "{err:?}");
    assert_eq!(
        lib.selection.count(),
        1,
        "a rejected click must not change the selection"
    );

    let err = run(&mut lib, "library.select", json!({"indices": "0"})).expect_err("array required");
    assert!(matches!(err, CommandError::BadParam(_)), "{err:?}");
}

#[test]
fn month_and_year_openers_validate_their_keys_and_limit_the_view() {
    let mut lib = library_with("month");
    let key = lib
        .groups()
        .first()
        .map(|g| g.key.clone())
        .expect("fixtures group into at least one month");
    assert_eq!(
        run(&mut lib, "month.open", json!({"key": key})).expect("open")["month"],
        key.as_str()
    );
    assert_eq!(lib.visible().len(), 2);
    run(&mut lib, "month.open", json!({"key": "1999-01"}))
        .expect("a month with no photos is still a valid filter");
    assert_eq!(lib.visible().len(), 0);
    run(&mut lib, "month.open", json!({"key": null})).expect("null clears the month");
    assert_eq!(lib.visible().len(), 2);

    let err =
        run(&mut lib, "month.open", json!({"key": "2016"})).expect_err("YYYY is not a month key");
    assert!(matches!(err, CommandError::BadParam(_)), "{err:?}");
    let err = run(&mut lib, "year.open", json!({"key": "2016-08"}))
        .expect_err("YYYY-MM is not a year key");
    assert!(matches!(err, CommandError::BadParam(_)), "{err:?}");

    let year = lib
        .groups()
        .first()
        .map(|g| g.key[..4].to_string())
        .expect("a year");
    run(&mut lib, "year.open", json!({"key": year.clone()})).expect("open a year");
    assert_eq!(lib.year, Some(year));
    assert_eq!(
        lib.view,
        ViewMode::Months,
        "opening a year shows its months"
    );
    assert_eq!(lib.visible().len(), 2);
}

#[test]
fn group_collapse_keeps_groups_independent() {
    let mut lib = library_with("collapse");
    // two months, so collapsing one must leave the other visible
    fixture::write_fixture(lib.folder(), "older.heic", BT709, SRGB, 0, false);
    let day = std::time::Duration::from_secs(86_400);
    let now = std::time::SystemTime::now();
    set_mtime(&lib.folder().join("hlg.heic"), now);
    set_mtime(&lib.folder().join("sdr.heic"), now - day * 400);
    set_mtime(&lib.folder().join("older.heic"), now - day * 900);
    lib.rescan();

    let keys: Vec<String> = lib.groups().iter().map(|g| g.key.clone()).collect();
    assert!(keys.len() >= 2, "need two months, got {keys:?}");
    let all = lib.visible().len();
    assert_eq!(all, 3);

    run(&mut lib, "group.toggle", json!({"key": keys[0]})).expect("collapse one");
    assert_eq!(
        lib.visible().len(),
        all - lib
            .groups()
            .iter()
            .find(|g| g.key == keys[0])
            .map(|g| g.indices.len())
            .unwrap_or(0)
    );

    run(&mut lib, "group.collapseAll", json!({})).expect("collapse all");
    assert_eq!(lib.visible().len(), 0);
    run(&mut lib, "group.expandAll", json!({})).expect("expand all");
    assert_eq!(lib.visible().len(), all);
}

#[test]
fn filters_and_sort_change_what_is_visible() {
    let mut lib = library_with("filter");
    fixture::write_fixture(lib.folder(), "clip.mp4", BT709, SRGB, 0, false);
    lib.rescan();
    assert_eq!(lib.entries.len(), 3);

    run(&mut lib, "filter.kind", json!({"kind": "video"})).expect("video only");
    assert_eq!(lib.order().len(), 1);
    run(&mut lib, "filter.kind", json!({"kind": "photo"})).expect("photo only");
    assert_eq!(lib.order().len(), 2);
    run(&mut lib, "filter.hdr", json!({"on": true})).expect("hdr only");
    assert_eq!(lib.order().len(), 1, "only the HLG file is HDR");
    run(&mut lib, "filter.clear", json!({})).expect("clear filters");
    assert_eq!(lib.order().len(), 3);

    run(
        &mut lib,
        "sort.set",
        json!({"key": "name", "direction": "natural"}),
    )
    .expect("sort by name");
    let names: Vec<String> = lib.order().iter().map(|&i| lib.entries[i].name()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "natural name order");

    run(
        &mut lib,
        "sort.set",
        json!({"key": "name", "direction": "reversed"}),
    )
    .expect("reverse");
    let reversed: Vec<String> = lib.order().iter().map(|&i| lib.entries[i].name()).collect();
    assert_eq!(
        reversed,
        sorted.iter().rev().cloned().collect::<Vec<String>>()
    );

    let err = run(&mut lib, "sort.set", json!({"key": "pixels"})).expect_err("not a sort key");
    assert!(matches!(err, CommandError::BadParam(_)), "{err:?}");
    assert_eq!(
        lib.sort,
        spectrum_core::SortKey::Name,
        "a rejected sort key keeps the previous one"
    );

    run(&mut lib, "filter.kind", json!({"kind": "video"})).expect("video only again");
    assert_eq!(lib.filter.kind, Some(spectrum_core::scan::MediaKind::Video));
    let err = run(&mut lib, "filter.kind", json!({"kind": "raw"})).expect_err("not a kind");
    assert!(matches!(err, CommandError::BadParam(_)), "{err:?}");
    assert_eq!(
        lib.filter.kind,
        Some(spectrum_core::scan::MediaKind::Video),
        "a rejected filter keeps the previous one"
    );
}

#[test]
fn edit_commands_require_exactly_one_selection() {
    let mut lib = library_with("edit-guard");
    let err = run(&mut lib, "edit.rotate", json!({"dir": -90})).expect_err("nothing selected");
    assert!(matches!(err, CommandError::Disabled(_)), "{err:?}");
    assert!(err.to_string().contains("no photo is selected"), "{err}");

    run(&mut lib, "library.select", json!({"indices": [0, 1]})).expect("two selected");
    let err = run(&mut lib, "edit.flipH", json!({})).expect_err("two selected");
    assert!(matches!(err, CommandError::Disabled(_)), "{err:?}");
    assert!(err.to_string().contains("exactly one"), "{err}");
    assert_eq!(
        lib.entries.iter().filter(|e| e.edit.is_some()).count(),
        0,
        "a disabled command wrote nothing"
    );

    run(&mut lib, "library.select", json!({"indices": [0]})).expect("one selected");
    let after = run(&mut lib, "edit.rotate", json!({"dir": -90})).expect("rotate");
    assert_eq!(after["rotation"], 270);
    assert_eq!(after["flipH"], false);
    assert_eq!(
        lib.entries[0]
            .edit
            .as_ref()
            .expect("entry carries the edit")
            .rotation,
        270
    );

    let flipped = run(&mut lib, "edit.flipH", json!({})).expect("flip");
    assert_eq!(flipped["flipH"], true);
    assert_eq!(
        flipped["rotation"], 90,
        "a flip mirrors the rotation: D4 group"
    );

    let cropped = run(
        &mut lib,
        "edit.crop",
        json!({"x": 0.1, "y": 0.2, "width": 0.3, "height": 0.4}),
    )
    .expect("crop");
    assert_eq!(cropped["crop"]["width"], 0.3);

    let err = run(
        &mut lib,
        "edit.crop",
        json!({"x": 0.5, "y": 0.0, "width": 0.9, "height": 0.5}),
    )
    .expect_err("crop runs past 1.0");
    assert!(matches!(err, CommandError::BadParam(_)), "{err:?}");
    let err = run(
        &mut lib,
        "edit.crop",
        json!({"x": 0.1, "y": 0.2, "width": 0.0, "height": 0.4}),
    )
    .expect_err("zero width");
    assert!(matches!(err, CommandError::BadParam(_)), "{err:?}");
    let err = run(&mut lib, "edit.rotate", json!({"dir": 45})).expect_err("only 90 and -90");
    assert!(matches!(err, CommandError::BadParam(_)), "{err:?}");

    let sidecar = lib.entries[0].path.with_extension("heic.xmp");
    assert!(sidecar.exists(), "the edit is stored in the sidecar");
    run(&mut lib, "edit.restore", json!({})).expect("restore");
    assert!(!sidecar.exists(), "restore removes the sidecar");
    assert!(lib.entries[0].edit.is_none());
}

#[test]
fn edits_never_touch_the_original_file() {
    let mut lib = library_with("edit-immutable");
    let path = lib.entries[0].path.clone();
    let before = std::fs::read(&path).expect("read original");
    run(&mut lib, "library.select", json!({"indices": [0]})).expect("select");
    run(&mut lib, "edit.rotate", json!({"dir": 90})).expect("rotate");
    run(
        &mut lib,
        "edit.crop",
        json!({"x": 0.25, "y": 0.25, "width": 0.5, "height": 0.5}),
    )
    .expect("crop");
    assert_eq!(
        std::fs::read(&path).expect("read after edits"),
        before,
        "the media file is byte-identical"
    );
}

#[test]
fn trash_moves_selected_files_and_rescans() {
    let mut lib = library_with("trash");
    let names: Vec<String> = lib.entries.iter().map(|e| e.name()).collect();
    assert_eq!(names.len(), 2);
    run(&mut lib, "library.select", json!({"indices": [0]})).expect("select one");
    let result = run(&mut lib, "photo.trash", json!({})).expect("trash");
    assert_eq!(result["moved"], 1);
    assert_eq!(result["total"], 1);
    assert!(
        result["status"]
            .as_str()
            .unwrap_or("")
            .contains("Trashed 1 of 1"),
        "{result}"
    );
    assert_eq!(
        lib.entries.len(),
        1,
        "the folder no longer holds the trashed file"
    );
    assert_eq!(lib.selection.count(), 0, "trashing clears the selection");
    assert!(
        lib.trash_root.join("files").exists(),
        "the trash directory holds the files"
    );

    let err = run(&mut lib, "photo.trash", json!({})).expect_err("nothing selected");
    assert!(matches!(err, CommandError::Disabled(_)), "{err:?}");
}

#[test]
fn status_clear_hides_the_status_bar() {
    let mut lib = library_with("status");
    lib.status.begin("Scanning");
    assert!(lib.status.is_visible());
    assert_eq!(
        run(&mut lib, "status.clear", json!({})).expect("clear")["visible"],
        false
    );
}

#[test]
fn a_non_object_parameter_is_rejected_not_panic() {
    let mut lib = library_with("bad-args");
    for args in [json!("text"), json!(7), json!([1, 2]), json!(true)] {
        let err = run(&mut lib, "view.set", args.clone())
            .expect_err(&format!("{} must be rejected", args));
        assert!(
            matches!(err, CommandError::BadParam(_)),
            "{err:?} for {args}"
        );
    }
    run(&mut lib, "hdr.toggle", json!(null))
        .expect("null parameters mean none, which hdr.toggle accepts");
}
