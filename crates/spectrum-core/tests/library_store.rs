//! The persisted folder list (`library_store`): a restart keeps the folders, and a folder that
//! vanished is reported instead of coming back empty.

use std::path::{Path, PathBuf};

use spectrum_core::library_store;
use spectrum_core::model::{FolderSlot, Library};
use spectrum_testkit::fixture;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("spectrum-store-{tag}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn store_in(dir: &Path) -> PathBuf {
    let path = library_store::path_in(dir);
    // `save` creates parents; a hand-written file needs the same directory first.
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    path
}

#[test]
fn a_saved_list_round_trips_with_the_active_folder() {
    let dir = temp_dir("roundtrip");
    let one = dir.join("one");
    let two = dir.join("two");
    std::fs::create_dir_all(&one).unwrap();
    std::fs::create_dir_all(&two).unwrap();

    let path = store_in(&dir);
    let folders = vec![
        FolderSlot {
            path: one.clone(),
            count: Some(7),
        },
        FolderSlot {
            path: two.clone(),
            count: None,
        },
    ];
    library_store::save(&path, &folders, 1).expect("save");
    let saved = library_store::load(&path).expect("load");
    assert_eq!(saved.folders.len(), 2);
    assert_eq!(saved.folders[0].path, one);
    assert_eq!(
        saved.folders[0].count,
        Some(7),
        "the sidebar count survives"
    );
    assert_eq!(saved.folders[1].count, None);
    assert_eq!(saved.active, 1);
    assert!(saved.dropped.is_empty());

    // The store lives under `<config>/spectrum/`, and no scratch file is left behind.
    assert_eq!(path, dir.join("spectrum").join("library.json"));
    assert!(!dir.join("spectrum").join("library.json.tmp").exists());
}

#[test]
fn a_missing_folder_is_dropped_and_named() {
    let dir = temp_dir("missing");
    let present = dir.join("present");
    std::fs::create_dir_all(&present).unwrap();
    let gone = dir.join("gone");

    let path = store_in(&dir);
    std::fs::write(
        path.as_path(),
        format!(
            "{{\"version\":{},\"active\":1,\"folders\":[{{\"path\":\"{}\"}},{{\"path\":\"{}\"}}]}}",
            library_store::LIBRARY_STORE_VERSION,
            present.display(),
            gone.display()
        ),
    )
    .unwrap();

    let saved = library_store::load(&path).expect("load");
    assert_eq!(saved.folders.len(), 1, "the vanished folder is not listed");
    assert_eq!(saved.folders[0].path, present);
    assert_eq!(saved.dropped, vec![gone.clone()]);
    assert_eq!(saved.active, 0, "active is clamped to the surviving list");
}

#[test]
fn duplicates_and_blank_entries_are_collapsed() {
    let dir = temp_dir("dupes");
    let one = dir.join("one");
    std::fs::create_dir_all(&one).unwrap();
    let path = store_in(&dir);
    std::fs::write(
        path.as_path(),
        format!(
            "{{\"version\":{},\"active\":0,\"folders\":[{{\"path\":\"{}\"}},{{\"path\":\"{}\"}},{{\"path\":\"\"}}]}}",
            library_store::LIBRARY_STORE_VERSION,
            one.display(),
            one.display()
        ),
    )
    .unwrap();
    let saved = library_store::load(&path).expect("load");
    assert_eq!(saved.folders.len(), 1);
    assert_eq!(saved.duplicates, 1);
    assert!(
        saved.dropped.is_empty(),
        "a blank entry is not a missing folder"
    );
}

#[test]
fn a_malformed_or_other_version_file_is_an_error_not_a_reset() {
    let dir = temp_dir("malformed");
    let path = store_in(&dir);

    std::fs::write(path.as_path(), b"{ not json").unwrap();
    let err = library_store::load(&path).expect_err("bad JSON must not read as empty");
    assert!(err.contains("not valid JSON"), "got {err}");
    assert!(
        err.contains(&path.display().to_string()),
        "the name is in {err}"
    );

    std::fs::write(
        path.as_path(),
        b"{\"version\":99,\"active\":0,\"folders\":[]}",
    )
    .unwrap();
    let err = library_store::load(&path).expect_err("a future version must not be guessed at");
    assert!(err.contains("version"), "got {err}");

    // A missing file is an empty library, not an error.
    std::fs::remove_file(&path).unwrap();
    let saved = library_store::load(&path).expect("no file is an empty list");
    assert_eq!(saved, spectrum_core::Saved::default());
}

#[test]
fn applying_the_store_rescans_so_the_counts_are_live() {
    let dir = temp_dir("apply");
    let media = dir.join("media");
    std::fs::create_dir_all(&media).unwrap();
    fixture::write_png_fixture(&media, "a.png", 40, 40, [10, 20, 30]);
    fixture::write_png_fixture(&media, "b.png", 40, 40, [10, 20, 30]);
    let other = dir.join("other");
    std::fs::create_dir_all(&other).unwrap();

    let path = store_in(&dir);
    library_store::save(
        &path,
        &[
            FolderSlot {
                path: other.clone(),
                count: Some(99),
            },
            FolderSlot {
                path: media.clone(),
                count: Some(99),
            },
        ],
        1,
    )
    .expect("save");

    let saved = library_store::load(&path).expect("load");
    let mut lib = Library::new(&other);
    library_store::apply(&mut lib, &saved);
    assert_eq!(lib.active, 1);
    assert_eq!(lib.folder(), media.as_path());
    assert_eq!(lib.entries.len(), 2, "apply rescans, so 99 is not trusted");
    assert_eq!(
        lib.folders[1].count,
        Some(2),
        "the sidebar shows the live count"
    );
}

#[test]
fn save_creates_the_directory_it_needs() {
    let dir = temp_dir("create");
    let path = dir.join("deep").join("nested").join("library.json");
    library_store::save(&path, &[], 0).expect("save creates parents");
    assert!(path.exists());
}
