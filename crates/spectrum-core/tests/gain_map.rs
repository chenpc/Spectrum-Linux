//! ISO 21496-1 gain map detection (JPEG APP2 URN segment).
//!
//! The macOS reference checks gain map auxiliary data first, so a file that carries a gain map must
//! be flagged HDR even when its ICC profile name says sRGB.

use spectrum_core::{hdr, jpeg};
use spectrum_testkit::fixture;

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("spectrum-core-{tag}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn fixture(_icc: Option<&str>, gain_map: bool) -> fixture::ExifFixture<'_> {
    fixture::ExifFixture {
        orientation: 1,
        colorspace: Some(1),
        custom_rendered: None,
        make: None,
        model: None,
        iso: None,
        exposure: None,
        f_number: None,
        focal_length: None,
        gain_map,
        cicp: None,
        dmnd: None,
        date_time_original: None,
    }
}

#[test]
fn app2_gain_map_is_detected_with_headroom() {
    let dir = temp_dir("gainmap");
    let path = fixture::write_jpeg_fixture_exif(&dir, "gainmap.jpg", None, &fixture(None, true));
    let info = jpeg::parse_jpeg(&std::fs::read(&path).unwrap()).unwrap();
    let gm = info.gain_map.as_ref().unwrap();
    assert_eq!(gm.urn, "urn:iso:std:iso:ts:21496:-1");
    assert_eq!(gm.headroom, Some((4, 1)));

    let signal = hdr::signal_for_jpeg(&info);
    assert_eq!(signal.source, hdr::SignalSource::GainMap);
    assert!(hdr::is_hdr_signal(&signal));
    assert_eq!(hdr::badge_label(Some(&signal), true), "HDR");
    assert_eq!(hdr::badge_label(Some(&signal), false), "SDR");
}

#[test]
fn gain_map_beats_icc_profile_name() {
    let dir = temp_dir("gainmap_vs_icc");
    let path = fixture::write_jpeg_fixture_exif(
        &dir,
        "both.jpg",
        Some("Rec. ITU-R BT.2100 HLG"),
        &fixture(None, true),
    );
    let info = jpeg::parse_jpeg(&std::fs::read(&path).unwrap()).unwrap();
    assert!(info.gain_map.is_some());
    assert_eq!(
        hdr::signal_for_jpeg(&info).source,
        hdr::SignalSource::GainMap
    );
}

#[test]
fn gain_map_flagged_even_when_icc_claims_srgb() {
    let dir = temp_dir("gainmap_mislabeled");
    let path = fixture::write_jpeg_fixture_exif(
        &dir,
        "mislabeled.jpg",
        Some("sRGB IEC61966-2.1"),
        &fixture(None, true),
    );
    let info = jpeg::parse_jpeg(&std::fs::read(&path).unwrap()).unwrap();
    let signal = hdr::signal_for_jpeg(&info);
    assert_eq!(signal.source, hdr::SignalSource::GainMap);
    assert!(hdr::is_hdr_signal(&signal));
}

#[test]
fn custom_rendered_still_works_without_app2() {
    let dir = temp_dir("gainmap_custom");
    let mut f = fixture(None, false);
    f.custom_rendered = Some(3);
    let path = fixture::write_jpeg_fixture_exif(&dir, "old_iphone.jpg", None, &f);
    let info = jpeg::parse_jpeg(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(info.gain_map, None);
    assert_eq!(
        hdr::signal_for_jpeg(&info).source,
        hdr::SignalSource::ExifCustomRendered
    );
    assert!(hdr::is_hdr_signal(&hdr::signal_for_jpeg(&info)));
}

#[test]
fn icc_only_file_is_unaffected() {
    let dir = temp_dir("gainmap_icc_only");
    let path = fixture::write_jpeg_fixture_exif(
        &dir,
        "hlg.jpg",
        Some("Rec. ITU-R BT.2100 HLG"),
        &fixture(None, false),
    );
    let info = jpeg::parse_jpeg(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(info.gain_map, None);
    assert_eq!(
        hdr::signal_for_jpeg(&info).source,
        hdr::SignalSource::IccProfileName
    );
    assert_eq!(
        hdr::badge_label(Some(&hdr::signal_for_jpeg(&info)), true),
        "HLG"
    );
}

#[test]
fn scan_reports_gain_map_source() {
    let dir = temp_dir("gainmap_scan");
    fixture::write_jpeg_fixture_exif(&dir, "a_gainmap.jpg", None, &fixture(None, true));
    fixture::write_jpeg_fixture_exif(&dir, "b_sdr.jpg", None, &fixture(None, false));
    let entries = spectrum_core::scan::scan_folder(&dir);
    assert_eq!(entries.len(), 2);
    assert_eq!(
        entries[0].signal.as_ref().unwrap().source,
        hdr::SignalSource::GainMap
    );
    assert_eq!(
        entries[1].signal.as_ref().unwrap().source,
        hdr::SignalSource::ExifColorSpace
    );
}
