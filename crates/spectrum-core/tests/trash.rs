use spectrum_core::trash::{delete_paths, DeleteOutcome};
use std::path::PathBuf;

const T: &str = "2026-10-09T12:00:00";

fn fresh(tag: &str) -> (PathBuf, PathBuf) {
    let root = PathBuf::from(format!("/tmp/spectrum-trash-tests/{tag}"));
    let trash = PathBuf::from(format!("/tmp/spectrum-trash-tests/{tag}-trash"));
    std::fs::remove_dir_all(&root).ok();
    std::fs::remove_dir_all(&trash).ok();
    std::fs::create_dir_all(&root).unwrap();
    (root, trash)
}

#[test]
fn delete_moves_the_file_and_writes_trashinfo() {
    let (dir, trash) = fresh("basic");
    std::fs::write(dir.join("a.png"), b"x").unwrap();

    let report = delete_paths(&[dir.join("a.png")], &trash, T);
    assert_eq!(report.len(), 1);
    let (src, outcome) = &report[0];
    assert_eq!(
        *outcome,
        DeleteOutcome::Trashed(trash.join("files").join("a.png"))
    );
    assert!(!src.exists());
    assert!(trash.join("files/a.png").exists());

    let info = std::fs::read_to_string(trash.join("info/a.png.trashinfo")).unwrap();
    assert_eq!(
        info,
        format!(
            "[Trash Info]\nPath={}\nTime={}\n",
            dir.join("a.png").display(),
            T
        )
    );
}

#[test]
fn name_collision_appends_a_counter() {
    let (dir, trash) = fresh("collision");
    std::fs::create_dir_all(trash.join("files")).unwrap();
    std::fs::write(trash.join("files/a.png"), b"old").unwrap();
    std::fs::write(dir.join("a.png"), b"new").unwrap();

    let report = delete_paths(&[dir.join("a.png")], &trash, T);
    assert_eq!(
        report[0].1,
        DeleteOutcome::Trashed(trash.join("files").join("a.png 1"))
    );
    assert_eq!(std::fs::read(trash.join("files/a.png 1")).unwrap(), b"new");
    assert!(trash.join("info/a.png 1.trashinfo").exists());
}

#[test]
fn falls_back_to_permanent_delete_when_the_trash_is_unavailable() {
    let (dir, _) = fresh("fallback");
    // A regular file cannot become a directory, so create_dir_all fails.
    let blocker = PathBuf::from("/tmp/spectrum-trash-tests/blocker-file");
    std::fs::create_dir_all("/tmp/spectrum-trash-tests").unwrap();
    std::fs::write(&blocker, b"x").unwrap();
    std::fs::write(dir.join("b.png"), b"x").unwrap();

    let report = delete_paths(&[dir.join("b.png")], &blocker.join("Trash"), T);
    assert_eq!(report[0].1, DeleteOutcome::Removed);
    assert!(!dir.join("b.png").exists());
}

#[test]
fn missing_file_is_reported_as_failed() {
    let (dir, trash) = fresh("missing");
    let report = delete_paths(&[dir.join("gone.png")], &trash, T);
    assert_eq!(report[0].1, DeleteOutcome::Failed("not found".to_string()));
}

#[test]
fn empty_input_produces_empty_report() {
    let (_, trash) = fresh("empty");
    assert!(delete_paths(&[], &trash, T).is_empty());
}

#[test]
fn default_root_follows_xdg_data_home() {
    std::env::set_var("XDG_DATA_HOME", "/tmp/spectrum-trash-tests/xdg");
    assert_eq!(
        spectrum_core::trash::default_trash_root(),
        PathBuf::from("/tmp/spectrum-trash-tests/xdg/Trash")
    );
    std::env::remove_var("XDG_DATA_HOME");
}

#[test]
fn iso8601_utc_matches_the_epoch_second() {
    assert_eq!(
        spectrum_core::trash::iso8601_utc(
            std::time::UNIX_EPOCH + std::time::Duration::from_secs(1791158400)
        ),
        "2026-10-05T00:00:00"
    );
    assert_eq!(
        spectrum_core::trash::iso8601_utc(
            std::time::UNIX_EPOCH + std::time::Duration::from_secs(1780272061)
        ),
        "2026-06-01T00:01:01"
    );
}
