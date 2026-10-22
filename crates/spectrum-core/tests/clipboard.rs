use spectrum_core::clipboard::{paste, rename_dir, set_clipboard, ClipboardMode};
use std::path::PathBuf;

fn fresh(tag: &str) -> (PathBuf, PathBuf) {
    let src = PathBuf::from(format!("/tmp/spectrum-clipboard-tests/{tag}-src"));
    let dst = PathBuf::from(format!("/tmp/spectrum-clipboard-tests/{tag}-dst"));
    for d in [&src, &dst] {
        std::fs::remove_dir_all(d).ok();
        std::fs::create_dir_all(d).unwrap();
    }
    (src, dst)
}

#[test]
fn copy_keeps_the_source() {
    let (src, dst) = fresh("copy");
    std::fs::write(src.join("a.png"), b"x").unwrap();
    let clip = set_clipboard(&[src.join("a.png")], ClipboardMode::Copy);

    let report = paste(&clip, &dst);
    assert_eq!(report[0].error, None);
    assert_eq!(report[0].dest, dst.join("a.png"));
    assert!(src.join("a.png").exists());
    assert!(dst.join("a.png").exists());
}

#[test]
fn cut_moves_the_source() {
    let (src, dst) = fresh("cut");
    std::fs::write(src.join("b.png"), b"x").unwrap();
    let clip = set_clipboard(&[src.join("b.png")], ClipboardMode::Cut);

    let report = paste(&clip, &dst);
    assert_eq!(report[0].error, None);
    assert!(!src.join("b.png").exists());
    assert!(dst.join("b.png").exists());
}

#[test]
fn paste_collision_uses_the_counter() {
    let (src, dst) = fresh("collision");
    std::fs::write(src.join("a.png"), b"new").unwrap();
    std::fs::write(dst.join("a.png"), b"old").unwrap();
    let clip = set_clipboard(&[src.join("a.png")], ClipboardMode::Copy);

    let report = paste(&clip, &dst);
    assert_eq!(report[0].dest, dst.join("a.png 1"));
    assert_eq!(std::fs::read(dst.join("a.png")).unwrap(), b"old");
    assert_eq!(std::fs::read(dst.join("a.png 1")).unwrap(), b"new");
}

#[test]
fn paste_into_a_missing_directory_is_reported_per_item() {
    let (src, _) = fresh("missingdest");
    std::fs::write(src.join("a.png"), b"x").unwrap();
    let clip = set_clipboard(&[src.join("a.png")], ClipboardMode::Copy);
    let report = paste(
        &clip,
        PathBuf::from("/tmp/spectrum-clipboard-tests/nope").as_path(),
    );
    assert!(report[0]
        .error
        .as_ref()
        .unwrap()
        .contains("not a directory"));
}

#[test]
fn rename_folder_returns_the_new_path() {
    let (dir, _) = fresh("rename");
    // rename_dir puts the folder in the parent directory under the new name
    std::fs::remove_dir_all("/tmp/spectrum-clipboard-tests/renamed").ok();
    let target = rename_dir(&dir, "renamed").unwrap();
    assert_eq!(
        target,
        PathBuf::from("/tmp/spectrum-clipboard-tests/renamed")
    );
    assert!(target.is_dir());
    assert!(!dir.exists());
}

#[test]
fn rename_rejects_empty_and_pathlike_names() {
    let (dir, _) = fresh("badname");
    assert_eq!(rename_dir(&dir, ""), Err("empty name".to_string()));
    assert_eq!(
        rename_dir(&dir, "a/b"),
        Err("invalid name: a/b".to_string())
    );
    std::fs::remove_dir_all("/tmp/spectrum-clipboard-tests/x").ok();
    assert!(rename_dir(&dir, "x").is_ok());
}
