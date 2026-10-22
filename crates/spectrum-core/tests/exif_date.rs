use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use spectrum_core::{import, jpeg};
use spectrum_testkit::fixture;

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

fn fresh(tag: &str) -> PathBuf {
    let dir = PathBuf::from(format!("/tmp/spectrum-exif-date-tests/{tag}"));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn set_mtime(path: &PathBuf, t: SystemTime) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(t)
        .unwrap();
}

#[test]
fn reads_datetime_original_and_maps_it_to_a_date_key() {
    let dir = fresh("read");
    let f = fixture::ExifFixture {
        orientation: 1,
        make: Some("Sony"),
        date_time_original: Some("2020:01:02 03:04:05"),
        ..fixture::ExifFixture::default()
    };
    let path = fixture::write_jpeg_fixture_exif(&dir, "camera.jpg", None, &f);
    let info = jpeg::parse_jpeg(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        info.exif.date_time_original.as_deref(),
        Some("2020:01:02 03:04:05")
    );
    assert_eq!(info.exif.date_key().as_deref(), Some("2020-01-02"));
}

#[test]
fn malformed_or_missing_dates_have_no_date_key() {
    use spectrum_core::jpeg::ExifData;
    for bad in ["", "2020:01", "2020/01/02 03:04:05", "xx20:01:02 03:04:05"] {
        let e = ExifData {
            date_time_original: Some(bad.to_string()),
            ..ExifData::default()
        };
        assert_eq!(e.date_key(), None, "bad value {bad:?}");
    }
    let e = ExifData::default();
    assert_eq!(e.date_key(), None);
}

#[test]
fn import_groups_by_exif_date_when_present() {
    let dir = fresh("exif_wins");
    // EXIF date 2020-01-02, but the file was written later (mtime 2023-11-14).
    let f = fixture::ExifFixture {
        orientation: 1,
        date_time_original: Some("2020:01:02 03:04:05"),
        ..fixture::ExifFixture::default()
    };
    let jpg = fixture::write_jpeg_fixture_exif(&dir, "photo.jpg", None, &f);
    set_mtime(&jpg, at(1_700_000_000));

    // PNG has no EXIF, so it falls back to mtime (2021-06-05).
    let png = fixture::write_png_fixture(&dir, "plain.png", 4, 4, [1, 2, 3]);
    set_mtime(&png, at(1_622_851_200));

    let model = import::ImportModel::scan(&dir);
    let keys: Vec<String> = model.groups.iter().map(|g| g.key.clone()).collect();
    assert_eq!(keys, vec!["2021-06-05", "2020-01-02"]);
    assert_eq!(model.items[0].date_key, "2021-06-05"); // plain.png (mtime)
    assert_eq!(model.items[1].date_key, "2020-01-02"); // photo.jpg (EXIF)
}

#[test]
fn a_jpeg_without_an_exif_date_falls_back_to_mtime() {
    let dir = fresh("fallback");
    let f = fixture::ExifFixture {
        orientation: 1,
        make: Some("Sony"),
        ..fixture::ExifFixture::default()
    };
    let jpg = fixture::write_jpeg_fixture_exif(&dir, "no_date.jpg", None, &f);
    set_mtime(&jpg, at(1_700_000_000)); // 2023-11-14
    let model = import::ImportModel::scan(&dir);
    assert_eq!(model.groups[0].key, "2023-11-14");
}

#[test]
fn exif_date_and_mtime_can_share_a_group() {
    let dir = fresh("shared_group");
    let f = fixture::ExifFixture {
        orientation: 1,
        date_time_original: Some("2021:06:05 09:00:00"),
        ..fixture::ExifFixture::default()
    };
    let jpg = fixture::write_jpeg_fixture_exif(&dir, "a.jpg", None, &f);
    set_mtime(&jpg, at(1_700_000_000));
    let png = fixture::write_png_fixture(&dir, "b.png", 4, 4, [1, 2, 3]);
    set_mtime(&png, at(1_622_851_200)); // 2021-06-05

    let model = import::ImportModel::scan(&dir);
    assert_eq!(model.groups.len(), 1);
    assert_eq!(model.group_label(0), "2021-06-05  ·  2");
    // name order inside the group
    let names: Vec<String> = model.groups[0]
        .indices
        .iter()
        .map(|&i| model.item_name(i))
        .collect();
    assert_eq!(names, vec!["a.jpg", "b.png"]);
}
