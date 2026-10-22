//! EXIF camera/lens metadata (`jpeg::ExifData`).

use spectrum_core::jpeg;
use spectrum_testkit::fixture;

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("spectrum-core-{tag}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn reads_make_model_iso_and_exposure() {
    let dir = temp_dir("exif_full");
    let f = fixture::ExifFixture {
        orientation: 1,
        colorspace: Some(1),
        custom_rendered: None,
        make: Some("Sony"),
        model: Some("ILCE-7RM2"),
        iso: Some(100),
        exposure: Some((1, 250)),
        f_number: Some((28, 10)),
        focal_length: Some((55, 1)),
        gain_map: false,
        cicp: None,
        dmnd: None,
        date_time_original: None,
    };
    let path = fixture::write_jpeg_fixture_exif(&dir, "camera.jpg", None, &f);
    let info = jpeg::parse_jpeg(&std::fs::read(&path).unwrap()).unwrap();
    let e = &info.exif;
    assert_eq!(e.make.as_deref(), Some("Sony"));
    assert_eq!(e.model.as_deref(), Some("ILCE-7RM2"));
    assert_eq!(e.iso, Some(100));
    assert_eq!(e.exposure_time, Some((1, 250)));
    assert_eq!(e.f_number, Some((28, 10)));
    assert_eq!(e.focal_length, Some((55, 1)));
    assert_eq!(e.shutter_speed().as_deref(), Some("1/250s"));
    assert_eq!(e.aperture().as_deref(), Some("f/2.8"));
    assert_eq!(e.focal_length().as_deref(), Some("55mm"));
    assert_eq!(
        e.summary().as_deref(),
        Some("Sony ILCE-7RM2 · 1/250s · f/2.8 · 55mm · ISO 100")
    );
}

#[test]
fn missing_tags_are_none_and_summary_is_none() {
    let dir = temp_dir("exif_empty");
    let f = fixture::ExifFixture {
        orientation: 6,
        colorspace: Some(1),
        ..fixture::ExifFixture::default()
    };
    let path = fixture::write_jpeg_fixture_exif(&dir, "plain.jpg", None, &f);
    let info = jpeg::parse_jpeg(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(info.exif, jpeg::ExifData::default());
    assert_eq!(info.exif.summary(), None);
    // orientation must still be read even without the metadata tags
    assert_eq!(info.orientation, 6);
}

#[test]
fn shutter_speed_formats_long_exposures() {
    let e = jpeg::ExifData {
        exposure_time: Some((1, 2)),
        ..jpeg::ExifData::default()
    };
    assert_eq!(e.shutter_speed().as_deref(), Some("1/2s"));
    let e = jpeg::ExifData {
        exposure_time: Some((2, 1)),
        ..jpeg::ExifData::default()
    };
    assert_eq!(e.shutter_speed().as_deref(), Some("2s"));
    let e = jpeg::ExifData {
        exposure_time: Some((5, 2)),
        ..jpeg::ExifData::default()
    };
    assert_eq!(e.shutter_speed().as_deref(), Some("2.5s"));
    // zero numerator (no exposure) must not divide by zero
    let e = jpeg::ExifData {
        exposure_time: Some((0, 1)),
        ..jpeg::ExifData::default()
    };
    assert_eq!(e.shutter_speed(), None);
}

#[test]
fn summary_only_includes_present_fields() {
    let e = jpeg::ExifData {
        model: Some("iPhone 15 Pro".to_string()),
        iso: Some(64),
        ..jpeg::ExifData::default()
    };
    assert_eq!(e.summary().as_deref(), Some("iPhone 15 Pro · ISO 64"));
}

#[test]
fn scan_carries_exif_metadata() {
    let dir = temp_dir("exif_scan");
    let f = fixture::ExifFixture {
        orientation: 1,
        colorspace: Some(1),
        make: Some("Canon"),
        model: Some("EOS R5"),
        iso: Some(200),
        ..fixture::ExifFixture::default()
    };
    fixture::write_jpeg_fixture_exif(&dir, "a_canon.jpg", None, &f);
    fixture::write_fixture(&dir, "b_heif.heic", 9, 18, 0, true);

    let entries = spectrum_core::scan::scan_folder(&dir);
    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries[0]
            .signal
            .as_ref()
            .unwrap()
            .exif
            .as_ref()
            .unwrap()
            .model
            .as_deref(),
        Some("EOS R5")
    );
    // HEIF path has no EXIF parsed here
    assert!(entries[1].signal.as_ref().unwrap().exif.is_none());
}
