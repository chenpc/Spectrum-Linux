//! The directory list the "Add Folder" browser offers.

use std::path::{Path, PathBuf};

use spectrum_core::folder_browser::{self, DirEntry};
use spectrum_testkit::fixture;

fn dir(tag: &str) -> PathBuf {
    let mut root = std::env::temp_dir();
    root.push(format!("spectrum-browser-{tag}"));
    std::fs::create_dir_all(&root).unwrap_or_default();
    root
}

fn touch(path: &Path) {
    std::fs::create_dir_all(path).unwrap_or_default();
}

fn names(entries: &[DirEntry]) -> Vec<&str> {
    entries.iter().map(|e| e.name.as_str()).collect()
}

#[test]
fn only_directories_are_offered_and_hidden_ones_are_skipped() {
    let root = dir("listing");
    touch(&root.join("photos"));
    touch(&root.join("Videos"));
    touch(&root.join(".hidden"));
    touch(&root.join("photos/nested"));
    fixture::write_png_fixture(&root, "loose.png", 8, 8, [10, 20, 30]);

    let entries = folder_browser::listing(&root).unwrap_or_else(|err| panic!("{err}"));
    assert_eq!(
        names(&entries),
        vec!["photos", "Videos"],
        "files and dot-prefixed directories are not things you can add to the library"
    );
    // Case-insensitive ordering, so "Videos" does not jump above "photos".
    assert_eq!(entries[0].name, "photos");
    assert_eq!(
        entries[0].path,
        root.join("photos"),
        "the entry carries the path the browser will open"
    );
}

#[test]
fn each_entry_counts_the_media_files_it_holds_directly() {
    let root = dir("counts");
    touch(&root.join("many"));
    touch(&root.join("empty"));
    touch(&root.join("many/nested"));
    for i in 0..3 {
        fixture::write_png_fixture(&root.join("many"), &format!("p{i}.png"), 6, 6, [1, 2, 3]);
    }
    fixture::write_png_fixture(&root.join("many/nested"), "deep.png", 6, 6, [4, 5, 6]);
    fixture::write_png_fixture(&root.join("empty"), "note.txt.md", 6, 6, [7, 8, 9]);

    let entries = folder_browser::listing(&root).unwrap_or_else(|err| panic!("{err}"));
    let counts: Vec<(String, usize)> = entries.iter().map(|e| (e.name.clone(), e.media)).collect();
    assert_eq!(
        counts,
        vec![("empty".to_string(), 0), ("many".to_string(), 3)],
        "the count is the directory's own files, one level deep — nested folders are not scanned"
    );
}

#[test]
fn an_unreadable_directory_reports_a_reason() {
    let missing = dir("missing").join("not-here");
    let err = folder_browser::listing(&missing)
        .err()
        .unwrap_or_else(|| panic!("{} does not exist, so listing must fail", missing.display()));
    assert!(
        err.contains("cannot read") && err.contains("not-here"),
        "the reason should name the directory: {err}"
    );
}

#[test]
fn parent_walks_up_and_stops_at_the_root() {
    let root = dir("parent");
    let nested = root.join("a").join("b");
    touch(&nested);
    assert_eq!(folder_browser::parent(&nested), Some(root.join("a")));
    assert_eq!(
        folder_browser::parent(&root),
        root.parent().map(|p| p.to_path_buf()),
    );
    assert_eq!(
        folder_browser::parent(Path::new("/")),
        None,
        "there is nothing above the filesystem root"
    );
}

#[test]
fn home_is_the_environment_directory_when_present() {
    let home = folder_browser::home();
    if let Some(from_env) = std::env::var_os("HOME") {
        assert_eq!(home, PathBuf::from(from_env));
        assert!(home.is_dir(), "HOME must name a real directory");
    } else {
        assert_eq!(home, PathBuf::from("."), "no HOME falls back to the cwd");
    }
}
