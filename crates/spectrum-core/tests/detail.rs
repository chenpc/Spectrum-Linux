//! The single-image view's commands (`detail.open`, `detail.close`, `detail.zoom`) and the rule that
//! it only shows while exactly one item is selected.

use std::path::PathBuf;

use serde_json::{json, Value};
use spectrum_core::command::{dispatch, CommandError};
use spectrum_core::model::{Library, DETAIL_MAX_SCALE, DETAIL_MIN_SCALE};
use spectrum_core::sort::SortKey;
use spectrum_testkit::fixture;

fn library(tag: &str, count: usize) -> Library {
    let dir = std::env::temp_dir().join(format!("spectrum-detail-{tag}"));
    std::fs::create_dir_all(&dir).unwrap();
    for i in 0..count {
        fixture::write_png_fixture(&dir, &format!("p{i:02}.png"), 60, 40, [200, 40, 40]);
    }
    let mut lib = Library::new(&dir);
    lib.sort = SortKey::Name;
    lib.rescan();
    lib
}

fn run(lib: &mut Library, id: &str, params: Value) -> Result<Value, CommandError> {
    dispatch(lib, id, &params)
}

#[test]
fn opening_needs_exactly_one_selected_file() {
    let mut lib = library("open", 3);
    let err = run(&mut lib, "detail.open", json!({})).expect_err("nothing is selected");
    assert!(matches!(err, CommandError::Disabled(_)), "got {err:?}");
    assert_eq!(lib.detail, None);

    let all = lib.visible();
    lib.select(&all[..2]);
    let err = run(&mut lib, "detail.open", json!({})).expect_err("two files is not one");
    assert!(matches!(err, CommandError::Disabled(_)), "got {err:?}");
    assert_eq!(lib.detail, None, "a rejected command changes nothing");

    lib.select(&[all[1]]);
    let answer = run(&mut lib, "detail.open", json!({})).expect("one file opens");
    assert_eq!(answer["index"], json!(all[1]));
    assert_eq!(lib.detail, Some(all[1]));
    assert_eq!(lib.detail_index(), Some(all[1]));
}

#[test]
fn multi_selecting_closes_the_detail_view_without_a_command() {
    let mut lib = library("multi", 3);
    let all = lib.visible();
    lib.select(&[all[0]]);
    run(&mut lib, "detail.open", json!({})).expect("open");
    // Selecting more than one file is the grid's own business; the detail view simply stops
    // reporting an index, so the screen goes back to the grid.
    lib.select(&all);
    assert_eq!(lib.detail_index(), None);
    assert!(
        lib.detail.is_some(),
        "the model keeps the file it was showing"
    );
    lib.select(&[all[0]]);
    assert_eq!(
        lib.detail_index(),
        Some(all[0]),
        "back to one file shows it again"
    );
}

#[test]
fn closing_clears_the_zoom_so_the_next_file_starts_fitted() {
    let mut lib = library("close", 2);
    let all = lib.visible();
    lib.select(&[all[0]]);
    run(&mut lib, "detail.open", json!({})).expect("open");
    run(&mut lib, "detail.zoom", json!({"action": "one"})).expect("zoom");
    assert_eq!(lib.detail_zoom, Some(1.0));

    let answer = run(&mut lib, "detail.close", json!({})).expect("close");
    assert_eq!(answer["closed"], json!(true));
    assert_eq!(lib.detail, None);
    assert_eq!(
        lib.detail_zoom, None,
        "a new file must not inherit the old zoom"
    );
    let answer = run(&mut lib, "detail.close", json!({})).expect("closing twice is fine");
    assert_eq!(answer["closed"], json!(false));
}

#[test]
fn zoom_continues_from_the_scale_the_canvas_is_showing() {
    let mut lib = library("zoom", 2);
    let all = lib.visible();
    lib.select(&[all[0]]);

    let err = run(&mut lib, "detail.zoom", json!({"action": "in"})).expect_err("nothing is open");
    assert!(matches!(err, CommandError::Disabled(_)), "got {err:?}");

    run(&mut lib, "detail.open", json!({})).expect("open");
    // Fit is a scale the registry cannot compute (it needs the canvas size), so the UI passes it.
    let answer = run(
        &mut lib,
        "detail.zoom",
        json!({"action": "in", "current": 0.4}),
    )
    .expect("in");
    let zoomed = answer["zoom"].as_f64().unwrap_or_default();
    assert!(
        (zoomed - 0.5).abs() < 1e-6,
        "zooming in from a 0.4 fit gives 0.5, got {zoomed}"
    );
    assert_eq!(lib.detail_zoom.map(|z| z as f64), Some(zoomed));

    let answer = run(&mut lib, "detail.zoom", json!({"action": "out"})).expect("out");
    let zoomed_out = answer["zoom"].as_f64().unwrap_or_default();
    assert!(
        (zoomed_out - 0.4).abs() < 1e-6,
        "zooming out applies to the stored zoom, not to `current`: {zoomed_out}"
    );

    let answer = run(&mut lib, "detail.zoom", json!({"action": "one"})).expect("1:1");
    assert_eq!(answer["zoom"], json!(1.0));

    let answer = run(&mut lib, "detail.zoom", json!({"action": "fit"})).expect("fit");
    assert_eq!(answer["zoom"], Value::Null);
    assert_eq!(answer["fit"], json!(true));
    assert_eq!(lib.detail_zoom, None, "fit means no explicit zoom");
}

#[test]
fn zoom_stays_inside_its_bounds() {
    let mut lib = library("bounds", 1);
    let vis = lib.visible();
    lib.select(&vis);
    run(&mut lib, "detail.open", json!({})).expect("open");
    for _ in 0..40 {
        run(
            &mut lib,
            "detail.zoom",
            json!({"action": "in", "current": 1.0}),
        )
        .expect("in");
    }
    assert_eq!(
        lib.detail_zoom,
        Some(DETAIL_MAX_SCALE),
        "repeated zoom-in stops at the bound instead of running off"
    );
    for _ in 0..60 {
        run(&mut lib, "detail.zoom", json!({"action": "out"})).expect("out");
    }
    assert_eq!(lib.detail_zoom, Some(DETAIL_MIN_SCALE));
    // Zooming in from the bottom bound moves off it, so the bound is a floor and not a stuck value.
    let answer = run(&mut lib, "detail.zoom", json!({"action": "in"})).expect("in from the floor");
    assert!(
        answer["zoom"].as_f64().unwrap_or_default() > DETAIL_MIN_SCALE as f64,
        "the floor is escapable: {:?}",
        answer["zoom"]
    );
}

#[test]
fn an_unknown_zoom_action_is_a_parameter_error() {
    let mut lib = library("bad", 1);
    let vis = lib.visible();
    lib.select(&vis);
    run(&mut lib, "detail.open", json!({})).expect("open");
    let err =
        run(&mut lib, "detail.zoom", json!({"action": "sideways"})).expect_err("unknown action");
    assert!(matches!(err, CommandError::BadParam(_)), "got {err:?}");
    assert_eq!(lib.detail_zoom, None, "the zoom is unchanged");
    let err = run(&mut lib, "detail.zoom", json!({})).expect_err("action is required");
    assert!(matches!(err, CommandError::BadParam(_)), "got {err:?}");
}

#[test]
fn a_rescan_that_drops_the_open_file_closes_the_detail_view() {
    let dir = std::env::temp_dir().join("spectrum-detail-rescan");
    std::fs::create_dir_all(&dir).unwrap();
    let only = fixture::write_png_fixture(&dir, "only.png", 60, 40, [10, 20, 30]);
    let mut lib = Library::new(&dir);
    lib.rescan();
    lib.select(&lib.visible());
    run(&mut lib, "detail.open", json!({})).expect("open");
    assert!(lib.detail.is_some());

    std::fs::remove_file(&only).unwrap();
    lib.rescan();
    assert_eq!(lib.entries.len(), 0);
    assert_eq!(
        lib.detail, None,
        "a detail file that no longer exists is not shown"
    );
    assert_eq!(lib.detail_zoom, None);
}

#[test]
fn detail_commands_never_touch_the_files() {
    let dir = std::env::temp_dir().join("spectrum-detail-files");
    std::fs::create_dir_all(&dir).unwrap();
    let path = fixture::write_png_fixture(&dir, "a.png", 60, 40, [10, 20, 30]);
    let before = std::fs::read(&path).unwrap_or_default();

    let mut lib = Library::new(&dir);
    lib.rescan();
    let vis = lib.visible();
    lib.select(&vis);
    for (id, params) in [
        ("detail.open", json!({})),
        ("detail.zoom", json!({"action": "in", "current": 1.0})),
        ("detail.zoom", json!({"action": "one"})),
        ("detail.zoom", json!({"action": "fit"})),
        ("detail.close", json!({})),
    ] {
        dispatch(&mut lib, id, &params).ok();
    }

    assert_eq!(std::fs::read(&path).unwrap_or_default(), before);
    assert!(
        !PathBuf::from(format!("{}.xmp", path.display())).exists(),
        "opening and zooming write no sidecar"
    );
    let sidecars: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|_| panic!("dir unreadable"))
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("xmp"))
        .collect();
    assert!(
        sidecars.is_empty(),
        "no sidecar is written for any file in the folder"
    );
}
