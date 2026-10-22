use spectrum_core::scan::{MediaEntry, MediaKind};
use spectrum_core::sort::{sort_entries, sort_entries_dir, SortDirection, SortKey};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

fn entry(name: &str, size: u64, secs: u64) -> MediaEntry {
    MediaEntry {
        path: PathBuf::from(name),
        kind: MediaKind::Photo,
        signal: None,
        edit: None,
        video: None,
        mtime: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs)),
        size,
    }
}

fn entries() -> Vec<MediaEntry> {
    // deliberately not in size order so the sort is what decides
    vec![
        entry("b.png", 200, 100),
        entry("a.png", 300, 200),
        entry("c.png", 100, 300),
    ]
}

#[test]
fn size_natural_is_largest_first() {
    assert_eq!(sort_entries(&entries(), SortKey::Size), vec![1, 0, 2]);
}

#[test]
fn size_reversed_is_smallest_first() {
    assert_eq!(
        sort_entries_dir(&entries(), SortKey::Size, SortDirection::Reversed),
        vec![2, 0, 1]
    );
}

#[test]
fn name_direction_flips_the_order() {
    assert_eq!(sort_entries(&entries(), SortKey::Name), vec![1, 0, 2]);
    assert_eq!(
        sort_entries_dir(&entries(), SortKey::Name, SortDirection::Reversed),
        vec![2, 0, 1]
    );
}

#[test]
fn date_natural_stays_newest_first() {
    assert_eq!(
        sort_entries(&entries(), SortKey::DateModified),
        vec![2, 1, 0]
    );
    assert_eq!(
        sort_entries_dir(&entries(), SortKey::DateModified, SortDirection::Reversed),
        vec![0, 1, 2]
    );
}

#[test]
fn equal_keys_fall_back_to_the_name_and_the_direction_flips_that_too() {
    let e = vec![entry("z.png", 100, 1), entry("a.png", 100, 2)];
    assert_eq!(sort_entries(&e, SortKey::Size), vec![1, 0]);
    assert_eq!(
        sort_entries_dir(&e, SortKey::Size, SortDirection::Reversed),
        vec![0, 1]
    );
}

#[test]
fn scan_folder_records_the_file_size() {
    let dir = std::path::Path::new("/tmp/spectrum-size-tests");
    std::fs::remove_dir_all(dir).ok();
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("small.png"), vec![1u8; 10]).unwrap();
    std::fs::write(dir.join("big.png"), vec![2u8; 40]).unwrap();

    let got = spectrum_core::scan::scan_folder(dir);
    assert_eq!(got.len(), 2);
    // scan_folder sorts by path, so big.png comes first even though it is larger
    assert_eq!(got[0].size, 40, "scan_folder sorts by path");
    assert_eq!(got[1].size, 10);
    assert_eq!(sort_entries(&got, SortKey::Size), vec![0, 1]);
}
