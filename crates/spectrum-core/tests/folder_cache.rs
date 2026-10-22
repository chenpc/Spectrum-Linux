use std::path::PathBuf;

use spectrum_core::cover::list_subfolders;
use spectrum_core::folder_cache::FolderListCache;
use spectrum_testkit::fixture;

fn dir(tag: &str) -> PathBuf {
    let d = PathBuf::from(format!("/tmp/spectrum-folder-cache-{tag}"));
    std::fs::create_dir_all(&d).unwrap();
    for e in std::fs::read_dir(&d).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            std::fs::remove_dir_all(&p).unwrap();
        } else {
            std::fs::remove_file(&p).unwrap();
        }
    }
    d
}

fn names(infos: &[spectrum_core::SubfolderInfo]) -> Vec<String> {
    infos.iter().map(|i| i.name.clone()).collect()
}

#[test]
fn first_call_scans_and_caches_so_a_new_directory_is_not_seen() {
    let d = dir("scan-then-cache");
    std::fs::create_dir_all(d.join("a")).unwrap();
    fixture::write_png_fixture(&d.join("a"), "cover.png", 4, 4, [255, 0, 0]);

    let mut cache = FolderListCache::new();
    assert_eq!(names(&cache.list(&d)), vec!["a"]);

    // A directory added after the scan is invisible until the cache is invalidated.
    std::fs::create_dir_all(d.join("b")).unwrap();
    fixture::write_png_fixture(&d.join("b"), "cover.png", 4, 4, [0, 0, 255]);
    assert!(cache.can_reuse(&d));
    assert_eq!(names(&cache.list(&d)), vec!["a"]);
    assert_eq!(names(&list_subfolders(&d)), vec!["a", "b"]);
}

#[test]
fn storing_entries_marks_the_path_as_scanned_this_session() {
    let d = dir("scanned-flag");
    std::fs::create_dir_all(&d).unwrap();
    let mut cache = FolderListCache::new();
    assert!(!cache.is_scanned(&d));
    cache.set_entries(&d, &list_subfolders(&d));
    assert!(cache.is_scanned(&d));
}

#[test]
fn invalidate_clears_the_cache_and_the_scanned_flag() {
    let d = dir("invalidate");
    std::fs::create_dir_all(d.join("a")).unwrap();
    fixture::write_png_fixture(&d.join("a"), "cover.png", 4, 4, [255, 0, 0]);

    let mut cache = FolderListCache::new();
    cache.list(&d);
    std::fs::create_dir_all(d.join("b")).unwrap();
    fixture::write_png_fixture(&d.join("b"), "cover.png", 4, 4, [0, 0, 255]);

    cache.invalidate(&d);
    assert!(!cache.is_scanned(&d));
    assert_eq!(names(&cache.list(&d)), vec!["a", "b"]);
}

#[test]
fn a_deleted_cover_forces_a_rescan_even_when_the_path_was_scanned() {
    let d = dir("deleted-cover");
    std::fs::create_dir_all(d.join("a")).unwrap();
    fixture::write_png_fixture(&d.join("a"), "cover.png", 4, 4, [255, 0, 0]);

    let mut cache = FolderListCache::new();
    cache.list(&d);
    assert!(cache.can_reuse(&d));

    std::fs::remove_file(d.join("a/cover.png")).unwrap();
    assert!(!cache.can_reuse(&d));
    // The rescan sees the directory but no cover, so the entry is kept with cover = None.
    let after = cache.list(&d);
    assert_eq!(names(&after), vec!["a"]);
    assert!(after[0].cover.is_none());
}

#[test]
fn entry_for_child_looks_up_a_cached_child_through_its_parent() {
    let d = dir("child-lookup");
    std::fs::create_dir_all(d.join("a")).unwrap();
    fixture::write_png_fixture(&d.join("a"), "cover.png", 4, 4, [255, 0, 0]);

    let mut cache = FolderListCache::new();
    cache.list(&d);
    let child = cache
        .entry_for_child(&d.join("a"), &d)
        .expect("cached child");
    assert_eq!(child.name, "a");
    assert!(cache.entry_for_child(&d.join("missing"), &d).is_none());
}

#[test]
fn clear_empties_memory_and_scanned_flags() {
    let d = dir("clear");
    std::fs::create_dir_all(d.join("a")).unwrap();
    fixture::write_png_fixture(&d.join("a"), "cover.png", 4, 4, [255, 0, 0]);

    let mut cache = FolderListCache::new();
    cache.list(&d);
    cache.clear();
    assert!(!cache.is_scanned(&d));
    assert!(cache.entries(&d).is_none());
}

#[test]
fn sorted_view_uses_cover_date_order_from_the_cache() {
    let d = dir("sorted");
    for sub in ["z-new", "a-old"] {
        std::fs::create_dir_all(d.join(sub)).unwrap();
        fixture::write_png_fixture(&d.join(sub), "cover.png", 4, 4, [255, 0, 0]);
    }
    set_mtime(&d.join("z-new/cover.png"), 1791158400); // 2026-10-05
    set_mtime(&d.join("a-old/cover.png"), 1780272000); // 2026-06-01

    let mut cache = FolderListCache::new();
    assert_eq!(names(&cache.sorted(&d)), vec!["z-new", "a-old"]);
    // The cached raw listing stays name-sorted, so the cache itself is order-independent.
    assert_eq!(names(&cache.entries(&d).unwrap()), vec!["a-old", "z-new"]);
}

fn set_mtime(path: &PathBuf, secs: u64) {
    let t = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs);
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(t)
        .unwrap();
}
