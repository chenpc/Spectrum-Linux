//! e2e guard for filename + folder-name search (macOS `SearchResultsView` parity).

use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::Duration;

use spectrum_core::search::{normalize_query, search, PHOTO_LIMIT};
use spectrum_testkit::fixture::write_fixture;

fn root() -> PathBuf {
    let dir = Path::new("/tmp/spectrum-search-fixtures").to_path_buf();
    std::fs::create_dir_all(&dir).unwrap();
    write_fixture(&dir, "hlg_photo.png", 9, 18, 0, true);
    write_fixture(&dir, "sdr_photo.png", 1, 13, 0, true);
    write_fixture(&dir, "hlg_notes.txt", 1, 13, 0, true); // not media: must not match
    write_fixture(&dir, ".hidden_hlg.png", 1, 13, 0, true); // hidden: must not match
    let sub = dir.join("hlg-sub");
    std::fs::create_dir_all(&sub).unwrap();
    write_fixture(&sub, "HlgNested.png", 9, 18, 0, true);
    write_fixture(&sub, "hlg_video.mp4", 9, 18, 0, true);
    dir
}

#[test]
fn query_shorter_than_two_chars_searches_nothing() {
    assert_eq!(normalize_query("a"), None);
    assert_eq!(normalize_query("  a "), None);
    assert_eq!(normalize_query("ab"), Some("ab".to_string()));
    let r = search(&root(), "h");
    assert!(r.photos.is_empty() && r.folders.is_empty());
}

#[test]
fn matching_is_case_insensitive_substring() {
    let r = search(&root(), "HLG");
    let names: Vec<String> = r.photos.iter().map(|h| h.name.clone()).collect();
    assert!(
        names.contains(&"hlg_photo.png".to_string()),
        "got {names:?}"
    );
    assert!(
        names.contains(&"HlgNested.png".to_string()),
        "got {names:?}"
    );
}

#[test]
fn walk_is_recursive_and_skips_hidden_files_and_non_media() {
    let r = search(&root(), "hlg");
    let names: Vec<String> = r.photos.iter().map(|h| h.name.clone()).collect();
    // nested file found (scan_folder is non-recursive, search is)
    assert!(names.iter().any(|n| n == "HlgNested.png"));
    assert!(
        !names.iter().any(|n| n.starts_with('.')),
        "hidden file matched: {names:?}"
    );
    assert!(
        !names.iter().any(|n| n.ends_with(".txt")),
        "non-media matched: {names:?}"
    );
}

#[test]
fn folder_names_match_and_are_deduplicated() {
    let r = search(&root(), "hlg");
    let folders: Vec<String> = r.folders.iter().map(|(_, n)| n.clone()).collect();
    assert_eq!(folders, vec!["hlg-sub".to_string()], "got {folders:?}");
}

#[test]
fn media_kind_is_reported_on_hits() {
    let r = search(&root(), "hlg");
    let video = r.photos.iter().find(|h| h.name == "hlg_video.mp4");
    assert!(video.is_some(), "no video hit in {:?}", r.photos);
    assert_eq!(video.unwrap().kind, spectrum_core::scan::MediaKind::Video);
}

#[test]
fn hits_are_capped_then_sorted_newest_first() {
    let dir = Path::new("/tmp/spectrum-search-cap-fixtures").to_path_buf();
    std::fs::create_dir_all(&dir).unwrap();
    for i in 0..PHOTO_LIMIT + 5 {
        write_fixture(&dir, &format!("match{i:03}.png"), 1, 13, 0, true);
        sleep(Duration::from_millis(3));
    }
    let r = search(&dir, "match");
    assert_eq!(r.photos.len(), PHOTO_LIMIT, "cap not applied");
    // newest-first: the last created file must be first
    assert_eq!(r.photos[0].name, format!("match{:03}.png", PHOTO_LIMIT + 4));
    assert!(
        r.photos.windows(2).all(|w| w[0].mtime >= w[1].mtime),
        "not sorted newest-first"
    );
}
