//! The app keeps the folder list across a restart: `folder.add` / `folder.remove` write the store
//! file immediately, and a fresh `AppState` reads it back before anything is drawn.

use std::path::{Path, PathBuf};

use serde_json::json;
use spectrum_app::app::AppState;
use spectrum_core::library_store;
use spectrum_testkit::fixture;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("spectrum-app-store-{tag}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A folder with two media files, so a rescan finds something countable.
fn media_dir(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::create_dir_all(&path).unwrap();
    fixture::write_png_fixture(&path, "one.png", 40, 40, [200, 30, 30]);
    fixture::write_png_fixture(&path, "two.png", 40, 40, [30, 30, 200]);
    path
}

fn store(dir: &Path) -> PathBuf {
    let path = library_store::path_in(dir);
    std::fs::create_dir_all(path.parent().unwrap_or(dir)).unwrap_or_default();
    path
}

#[test]
fn adding_a_folder_writes_the_store_immediately() {
    let dir = temp_dir("add");
    let first = media_dir(&dir, "first");
    let second = media_dir(&dir, "second");
    let path = store(&dir);

    let mut state = AppState::new(first.clone());
    state.set_store(Some(path.clone()));
    assert!(state.run("folder.add", json!({"path": second.to_string_lossy()})));

    let saved = library_store::load(&path).expect("the store is readable");
    assert_eq!(saved.folders.len(), 2);
    assert_eq!(saved.folders[1].path, second);
    assert_eq!(saved.active, 1, "the newly added folder is the active one");
    assert_eq!(
        state.notice, None,
        "a successful change must not leave an error up"
    );
}

#[test]
fn a_restart_reads_the_store_back_before_drawing() {
    let dir = temp_dir("restart");
    let first = media_dir(&dir, "first");
    let second = media_dir(&dir, "second");
    let path = store(&dir);

    let mut state = AppState::new(first.clone());
    state.set_store(Some(path.clone()));
    state.run("folder.add", json!({"path": second.to_string_lossy()}));

    // A new process: the command line names an unrelated folder, the store wins.
    let unrelated = temp_dir("restart-unrelated");
    let mut restarted = AppState::new(unrelated.clone());
    restarted.set_store(Some(path.clone()));
    let saved = restarted.restore_store().expect("the store loads");
    assert_eq!(saved.folders.len(), 2);
    assert_eq!(restarted.lib.folders.len(), 2);
    assert_eq!(restarted.lib.folder(), second.as_path());
    assert_eq!(
        restarted.lib.entries.len(),
        2,
        "the store's folder is scanned"
    );
    assert_eq!(
        restarted.lib.folders[1].count,
        Some(2),
        "the sidebar count comes from the rescan"
    );
}

#[test]
fn a_folder_that_disappeared_is_reported_not_listed() {
    let dir = temp_dir("gone");
    let present = media_dir(&dir, "present");
    let gone = dir.join("unmounted");
    let command_line = media_dir(&dir, "cli");
    let path = store(&dir);
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

    let mut state = AppState::new(command_line);
    state.set_store(Some(path.clone()));
    let saved = state.restore_store().expect("load succeeds");
    assert_eq!(saved.dropped.len(), 1);
    assert_eq!(state.lib.folders.len(), 1);
    assert_eq!(state.lib.folder(), present.as_path());
    let notice = state.notice.clone().unwrap_or_default();
    assert!(notice.contains("Skipped 1"), "got {notice}");
    assert!(
        notice.contains(&gone.display().to_string()),
        "the missing path is named: {notice}"
    );
}

#[test]
fn a_broken_store_keeps_the_command_line_folder() {
    let dir = temp_dir("broken");
    let command_line = media_dir(&dir, "cli");
    let path = store(&dir);
    std::fs::write(path.as_path(), b"{ version: 1").unwrap();

    let mut state = AppState::new(command_line.clone());
    state.set_store(Some(path.clone()));
    let err = state
        .restore_store()
        .expect_err("a corrupt store must not read as empty");
    assert!(err.contains("not valid JSON"), "got {err}");
    assert_eq!(
        state.lib.folder(),
        command_line.as_path(),
        "the command-line folder is still what we show"
    );
    assert_eq!(state.lib.entries.len(), 2);
}

#[test]
fn removing_a_folder_rewrites_the_store_and_touches_no_files() {
    let dir = temp_dir("remove");
    let first = media_dir(&dir, "first");
    let second = media_dir(&dir, "second");
    let path = store(&dir);

    let mut state = AppState::new(first.clone());
    state.set_store(Some(path.clone()));
    state.run("folder.add", json!({"path": second.to_string_lossy()}));
    assert!(state.run("folder.remove", json!({"index": 0})));

    let saved = library_store::load(&path).expect("the store is readable");
    assert_eq!(saved.folders.len(), 1);
    assert_eq!(saved.folders[0].path, second);
    assert!(
        first.join("one.png").exists(),
        "removing never deletes files"
    );
    assert!(first.join("two.png").exists());
}

#[test]
fn without_a_store_path_nothing_is_written() {
    let dir = temp_dir("nostore");
    let first = media_dir(&dir, "first");
    let second = media_dir(&dir, "second");

    let mut state = AppState::new(first);
    assert!(
        state.store_path().is_none(),
        "tests default to no persistence"
    );
    assert!(state.run("folder.add", json!({"path": second})));
    assert_eq!(
        state.save_store(),
        None,
        "no path means no attempt to write"
    );
    assert!(
        !library_store::path_in(&dir).exists(),
        "no store file was created"
    );
}
