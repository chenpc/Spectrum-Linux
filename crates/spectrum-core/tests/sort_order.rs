//! Display order (`sort::sort_entries`).

use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use spectrum_core::{sort::SortKey, sort_entries};
use spectrum_testkit::fixture;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("spectrum-core-{tag}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn set_mtime(dir: &Path, name: &str, secs: u64) {
    let f = File::options().write(true).open(dir.join(name)).unwrap();
    f.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
        .unwrap();
}

fn names_in_order(entries: &[spectrum_core::MediaEntry], key: SortKey) -> Vec<String> {
    sort_entries(entries, key)
        .into_iter()
        .map(|i| entries[i].name())
        .collect()
}

#[test]
fn name_sort_is_case_insensitive() {
    let dir = temp_dir("sort_name");
    fixture::write_png_fixture(&dir, "B.png", 4, 4, [0, 0, 0]);
    fixture::write_png_fixture(&dir, "a.png", 4, 4, [0, 0, 0]);
    fixture::write_png_fixture(&dir, "C.png", 4, 4, [0, 0, 0]);
    let entries = spectrum_core::scan::scan_folder(&dir);
    assert_eq!(
        names_in_order(&entries, SortKey::Name),
        ["a.png", "B.png", "C.png"]
    );
}

#[test]
fn date_sort_is_newest_first() {
    let dir = temp_dir("sort_date");
    fixture::write_png_fixture(&dir, "a.png", 4, 4, [0, 0, 0]);
    fixture::write_png_fixture(&dir, "b.png", 4, 4, [0, 0, 0]);
    fixture::write_png_fixture(&dir, "c.png", 4, 4, [0, 0, 0]);
    // deliberately different from name order
    set_mtime(&dir, "a.png", 100);
    set_mtime(&dir, "b.png", 300);
    set_mtime(&dir, "c.png", 200);

    let entries = spectrum_core::scan::scan_folder(&dir);
    assert_eq!(
        names_in_order(&entries, SortKey::DateModified),
        ["b.png", "c.png", "a.png"]
    );
    // the same entries under Name order are still name-sorted -> the two keys really differ
    assert_eq!(
        names_in_order(&entries, SortKey::Name),
        ["a.png", "b.png", "c.png"]
    );
}

#[test]
fn kind_sort_puts_photos_before_videos() {
    let dir = temp_dir("sort_kind");
    fixture::write_png_fixture(&dir, "z_photo.png", 4, 4, [0, 0, 0]);
    fixture::write_png_fixture(&dir, "a_video.mp4", 4, 4, [0, 0, 0]);
    fixture::write_png_fixture(&dir, "m_photo.png", 4, 4, [0, 0, 0]);

    let entries = spectrum_core::scan::scan_folder(&dir);
    assert_eq!(
        names_in_order(&entries, SortKey::Kind),
        ["m_photo.png", "z_photo.png", "a_video.mp4"]
    );
}

#[test]
fn transfer_sort_puts_hdr_first() {
    let dir = temp_dir("sort_transfer");
    // header-only fixtures: glycin cannot decode them, but the signal is still read
    fixture::write_fixture(&dir, "z_hlg.heic", 9, 18, 0, true);
    fixture::write_jpeg_fixture(&dir, "a_sdr.jpg", None, Some(1), None, 1);
    fixture::write_jpeg_fixture(
        &dir,
        "b_hlg.jpg",
        Some("Rec. ITU-R BT.2100 HLG"),
        Some(1),
        None,
        1,
    );

    let entries = spectrum_core::scan::scan_folder(&dir);
    assert_eq!(
        names_in_order(&entries, SortKey::Transfer),
        ["b_hlg.jpg", "z_hlg.heic", "a_sdr.jpg"]
    );
}

#[test]
fn ties_fall_back_to_name() {
    let dir = temp_dir("sort_ties");
    fixture::write_png_fixture(&dir, "b.png", 4, 4, [0, 0, 0]);
    fixture::write_png_fixture(&dir, "a.png", 4, 4, [0, 0, 0]);
    set_mtime(&dir, "b.png", 500);
    set_mtime(&dir, "a.png", 500);

    let entries = spectrum_core::scan::scan_folder(&dir);
    assert_eq!(
        names_in_order(&entries, SortKey::DateModified),
        ["a.png", "b.png"]
    );
}
