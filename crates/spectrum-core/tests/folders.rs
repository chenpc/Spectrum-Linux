//! The sidebar is a folder list, so adding, opening, and removing folders is model-level behaviour
//! with command-level entry points.

use serde_json::json;
use spectrum_core::command::{dispatch, CommandError};
use spectrum_core::Library;
use spectrum_testkit::fixture;

const BT2020: u16 = 9;
const BT709: u16 = 1;
const SRGB: u16 = 13;
const HLG: u16 = 18;

fn dir(tag: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("spectrum-core-folders-{tag}"));
    std::fs::create_dir_all(&path).unwrap();
    for entry in std::fs::read_dir(&path).unwrap() {
        let file = entry.unwrap().path();
        if file.extension().is_some_and(|e| e == "xmp") {
            std::fs::remove_file(file).unwrap();
        }
    }
    path
}

fn run(
    lib: &mut Library,
    id: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, CommandError> {
    dispatch(lib, id, &args)
}

#[test]
fn a_new_library_holds_one_folder_with_a_count() {
    let a = dir("one");
    fixture::write_fixture(&a, "x.heic", BT2020, HLG, 0, true);
    let lib = Library::new(&a);

    assert_eq!(lib.folders.len(), 1);
    assert_eq!(lib.folder(), a.as_path());
    assert_eq!(
        lib.folders[0].count,
        Some(1),
        "the folder scanned at construction"
    );
    assert_eq!(lib.folders[0].label(), "spectrum-core-folders-one");
}

#[test]
fn adding_a_folder_opens_it_and_keeps_the_first_in_the_list() {
    let a = dir("two-a");
    let b = dir("two-b");
    fixture::write_fixture(&a, "a1.heic", BT709, SRGB, 0, false);
    fixture::write_fixture(&a, "a2.heic", BT709, SRGB, 0, false);
    fixture::write_fixture(&b, "b1.heic", BT2020, HLG, 0, true);

    let mut lib = Library::new(&a);
    let index = run(
        &mut lib,
        "folder.add",
        json!({"path": b.display().to_string()}),
    )
    .expect("add");
    assert_eq!(index["index"], 1);
    assert_eq!(lib.active, 1, "adding opens the new folder");
    assert_eq!(lib.entries.len(), 1);
    assert_eq!(lib.entries[0].name(), "b1.heic");
    assert_eq!(lib.folders.len(), 2);
    assert_eq!(
        lib.folders[0].count,
        Some(2),
        "the first folder keeps its count"
    );
}

#[test]
fn switching_folders_rescans_and_drops_selections_that_no_longer_exist() {
    let a = dir("three-a");
    let b = dir("three-b");
    fixture::write_fixture(&a, "a1.heic", BT709, SRGB, 0, false);
    fixture::write_fixture(&a, "a2.heic", BT709, SRGB, 0, false);
    fixture::write_fixture(&b, "b1.heic", BT2020, HLG, 0, true);

    let mut lib = Library::new(&a);
    run(&mut lib, "library.select", json!({"indices": [0, 1]})).expect("select both in a");
    assert_eq!(lib.selection.count(), 2);

    run(
        &mut lib,
        "folder.add",
        json!({"path": b.display().to_string()}),
    )
    .expect("add");
    assert_eq!(lib.entries.len(), 1, "b has one file");
    assert!(
        lib.selection.count() <= 1,
        "indices past the new folder's list are dropped, not kept stale"
    );

    run(&mut lib, "folder.open", json!({"index": 0})).expect("open a again");
    assert_eq!(lib.folder(), a.as_path());
    assert_eq!(lib.entries.len(), 2);
}

#[test]
fn bad_folder_arguments_are_rejected_and_change_nothing() {
    let a = dir("four-a");
    let b = dir("four-b");
    fixture::write_fixture(&a, "a1.heic", BT709, SRGB, 0, false);
    fixture::write_fixture(&b, "b1.heic", BT709, SRGB, 0, false);

    let mut lib = Library::new(&a);

    let err = run(
        &mut lib,
        "folder.add",
        json!({"path": a.display().to_string()}),
    )
    .expect_err("the same folder twice");
    assert!(matches!(err, CommandError::BadParam(_)), "{err:?}");

    let missing = std::env::temp_dir().join("spectrum-core-folders-does-not-exist");
    let err = run(
        &mut lib,
        "folder.add",
        json!({"path": missing.display().to_string()}),
    )
    .expect_err("not a directory");
    assert!(matches!(err, CommandError::Failed(_)), "{err:?}");

    let err = run(&mut lib, "folder.add", json!({})).expect_err("no path");
    assert!(matches!(err, CommandError::BadParam(_)), "{err:?}");

    let err = run(&mut lib, "folder.open", json!({"index": 7})).expect_err("past the list");
    assert!(matches!(err, CommandError::BadParam(_)), "{err:?}");

    assert_eq!(lib.folders.len(), 1, "nothing was added");
    assert_eq!(lib.active, 0, "and nothing was opened");
}

#[test]
fn removing_a_folder_leaves_the_files_on_disk() {
    let a = dir("five-a");
    let b = dir("five-b");
    let a_file = fixture::write_fixture(&a, "a1.heic", BT709, SRGB, 0, false);
    let b_file = fixture::write_fixture(&b, "b1.heic", BT2020, HLG, 0, true);

    let mut lib = Library::new(&a);
    run(
        &mut lib,
        "folder.add",
        json!({"path": b.display().to_string()}),
    )
    .expect("add");
    assert_eq!(lib.folder(), b.as_path());

    run(&mut lib, "folder.remove", json!({"index": 1})).expect("remove the active folder");
    assert_eq!(lib.folders.len(), 1);
    assert_eq!(
        lib.folder(),
        a.as_path(),
        "the remaining folder became active"
    );
    assert_eq!(lib.entries.len(), 1);
    assert!(
        a_file.exists() && b_file.exists(),
        "removing is not deleting"
    );

    run(&mut lib, "folder.remove", json!({"index": 0})).expect("remove the last folder");
    assert_eq!(lib.folders.len(), 0);
    assert!(lib.entries.is_empty(), "no folder means no entries");
    assert!(a_file.exists(), "still not deleting");

    let err = run(&mut lib, "folder.remove", json!({"index": 0})).expect_err("list is empty");
    assert!(
        matches!(err, CommandError::Disabled(_)),
        "guard, not a panic: {err:?}"
    );
    assert!(err.to_string().contains("no folders"), "{err}");
}
