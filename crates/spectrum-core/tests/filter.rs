//! Display filter (`filter_entries`) and its interaction with sorting.

use std::fs::File;
use std::time::{Duration, SystemTime};

use spectrum_core::{scan, sort::SortKey, Filter, MediaKind};
use spectrum_testkit::fixture;

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("spectrum-core-{tag}"));
    // Start clean: a leftover fixture from an earlier run changes the scan result.
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn set_mtime(dir: &std::path::Path, name: &str, secs: u64) {
    let t = SystemTime::UNIX_EPOCH + Duration::from_secs(secs);
    File::options()
        .write(true)
        .open(dir.join(name))
        .unwrap()
        .set_modified(t)
        .unwrap();
}

#[test]
fn kind_filter_keeps_only_matching_entries() {
    let dir = temp_dir("filter_kind");
    fixture::write_png_fixture(&dir, "a_photo.png", 8, 6, [1, 2, 3]);
    fixture::write_fixture(&dir, "b_video.mp4", 9, 18, 0, true);
    fixture::write_png_fixture(&dir, "c_photo.png", 8, 6, [1, 2, 3]);

    let entries = scan::scan_folder(&dir);
    assert_eq!(entries.len(), 3);

    let photos = Filter {
        kind: Some(MediaKind::Photo),
        hdr_only: false,
        transfer: None,
        month: None,
    };
    assert_eq!(
        spectrum_core::filter::filter_entries(&entries, &photos),
        vec![0, 2]
    );

    let videos = Filter {
        kind: Some(MediaKind::Video),
        hdr_only: false,
        transfer: None,
        month: None,
    };
    assert_eq!(
        spectrum_core::filter::filter_entries(&entries, &videos),
        vec![1]
    );

    assert_eq!(
        spectrum_core::filter::filter_entries(&entries, &Filter::default()),
        vec![0, 1, 2]
    );
}

#[test]
fn hdr_only_filter_uses_the_signal_not_the_pixels() {
    let dir = temp_dir("filter_hdr");
    // signalled HDR
    fixture::write_jpeg_fixture(
        &dir,
        "a_hdr.jpg",
        Some("Rec. ITU-R BT.2100 HLG"),
        Some(1),
        None,
        1,
    );
    // mislabeled: pixels are HLG but the ICC name claims sRGB -> must not pass
    fixture::write_jpeg_fixture(
        &dir,
        "b_mislabeled.jpg",
        Some("sRGB IEC61966-2.1"),
        Some(1),
        None,
        1,
    );
    fixture::write_png_fixture(&dir, "c_plain.png", 8, 6, [1, 2, 3]);

    let entries = scan::scan_folder(&dir);
    let f = Filter {
        kind: None,
        hdr_only: true,
        transfer: None,
        month: None,
    };
    assert_eq!(spectrum_core::filter::filter_entries(&entries, &f), vec![0]);
    assert_eq!(f.label(), "all kinds · HDR only");
}

#[test]
fn filter_and_sort_compose() {
    let dir = temp_dir("filter_sort");
    // Name order: a, b, c, d. mtimes make Date order d, c, b, a.
    fixture::write_png_fixture(&dir, "a_photo.png", 8, 6, [1, 2, 3]);
    fixture::write_jpeg_fixture(
        &dir,
        "b_hdr.jpg",
        Some("Rec. ITU-R BT.2100 HLG"),
        Some(1),
        None,
        1,
    );
    fixture::write_png_fixture(&dir, "c_photo.png", 8, 6, [1, 2, 3]);
    fixture::write_fixture(&dir, "d_video.mp4", 9, 16, 0, true); // video, PQ
    set_mtime(&dir, "a_photo.png", 100);
    set_mtime(&dir, "b_hdr.jpg", 200);
    set_mtime(&dir, "c_photo.png", 300);
    set_mtime(&dir, "d_video.mp4", 400);

    let entries = scan::scan_folder(&dir);
    assert_eq!(entries[3].kind, MediaKind::Video);
    let f = Filter {
        kind: Some(MediaKind::Photo),
        hdr_only: false,
        transfer: None,
        month: None,
    };
    let idx = spectrum_core::filter::filter_entries(&entries, &f);
    // photos only (a, b, c), newest first -> c (300), b (200), a (100)
    assert_eq!(
        spectrum_core::sort::sort_indices(
            &entries,
            SortKey::DateModified,
            &idx,
            spectrum_core::sort::SortDirection::Natural
        ),
        vec![2, 1, 0]
    );

    // HDR-only + name order -> b, d
    let f2 = Filter {
        kind: None,
        hdr_only: true,
        transfer: None,
        month: None,
    };
    let idx2 = spectrum_core::filter::filter_entries(&entries, &f2);
    assert_eq!(
        spectrum_core::sort::sort_indices(
            &entries,
            SortKey::Name,
            &idx2,
            spectrum_core::sort::SortDirection::Natural
        ),
        vec![1, 3]
    );
}

#[test]
fn transfer_filter_separates_hlg_and_pq() {
    let dir = temp_dir("filter_transfer");
    // gain map: HDR but no transfer code (Unspecified)
    fixture::write_jpeg_fixture(&dir, "a_gainmap.jpg", None, Some(1), Some(3), 1);
    fixture::write_jpeg_fixture(
        &dir,
        "b_hlg.jpg",
        Some("Rec. ITU-R BT.2100 HLG"),
        Some(1),
        None,
        1,
    );
    fixture::write_jpeg_fixture(
        &dir,
        "c_pq.jpg",
        Some("Rec. ITU-R BT.2100 PQ"),
        Some(1),
        None,
        1,
    );
    fixture::write_png_fixture(&dir, "d_plain.png", 8, 6, [1, 2, 3]);

    let entries = scan::scan_folder(&dir);
    let hlg = Filter {
        kind: None,
        hdr_only: false,
        transfer: Some(spectrum_core::TransferFunction::Hlg),
        month: None,
    };
    assert_eq!(
        spectrum_core::filter::filter_entries(&entries, &hlg),
        vec![1]
    );
    assert_eq!(hlg.label(), "all kinds · HLG");

    let pq = Filter {
        kind: None,
        hdr_only: false,
        transfer: Some(spectrum_core::TransferFunction::Pq),
        month: None,
    };
    assert_eq!(
        spectrum_core::filter::filter_entries(&entries, &pq),
        vec![2]
    );

    // HDR only keeps all three signalled files, including the gain-map one with no transfer code.
    let hdr = Filter {
        kind: None,
        hdr_only: true,
        transfer: None,
        month: None,
    };
    assert_eq!(
        spectrum_core::filter::filter_entries(&entries, &hdr),
        vec![0, 1, 2]
    );
    assert_eq!(hdr.label(), "all kinds · HDR only");
}

#[test]
fn month_filter_keeps_only_matching_month() {
    let dir = temp_dir("filter_month");
    fixture::write_png_fixture(&dir, "a_oct.png", 8, 6, [1, 2, 3]);
    fixture::write_png_fixture(&dir, "b_jun.png", 8, 6, [1, 2, 3]);
    set_mtime(&dir, "a_oct.png", 1791158400); // 2026-10-05 UTC
    set_mtime(&dir, "b_jun.png", 1780272000); // 2026-06-01 UTC

    let entries = scan::scan_folder(&dir);
    let f = Filter {
        kind: None,
        hdr_only: false,
        transfer: None,
        month: Some("2026-10".to_string()),
    };
    assert_eq!(spectrum_core::filter::filter_entries(&entries, &f), vec![0]);
    assert_eq!(f.label(), "all kinds · 2026-10");

    // An entry with no mtime never matches a month filter.
    let mut no_time = entries[0].clone();
    no_time.mtime = None;
    assert!(!f.matches(&no_time));
}
