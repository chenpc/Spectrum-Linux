//! EXIF / ICC transfer-function detection for JPEG (`detect_signal`).
//!
//! Priority order mirrors the macOS reference `ImagePreloadCache.detectHDR`:
//! ICC profile name → EXIF CustomRendered == 3 → EXIF ColorSpace.

use spectrum_core::{hdr, scan};
use spectrum_testkit::fixture;

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("spectrum-core-{tag}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const SRGB: hdr::TransferFunction = hdr::TransferFunction::Srgb;
const HLG: hdr::TransferFunction = hdr::TransferFunction::Hlg;
const PQ: hdr::TransferFunction = hdr::TransferFunction::Pq;

#[test]
fn srgb_jpeg_is_sdr() {
    let dir = temp_dir("jpeg_srgb");
    let path = fixture::write_jpeg_fixture(&dir, "sdr.jpg", None, Some(1), None, 1);
    let signal = scan::detect_signal(&path).expect("EXIF found");
    assert_eq!(signal.transfer, SRGB);
    assert_eq!(signal.source, hdr::SignalSource::ExifColorSpace);
    assert!(!hdr::is_hdr_signal(&signal));
    assert_eq!(
        hdr::display_mode(Some(&signal), true),
        hdr::DisplayMode::Sdr
    );
}

#[test]
fn hlg_icc_profile_is_hdr() {
    let dir = temp_dir("jpeg_hlg");
    let path = fixture::write_jpeg_fixture(
        &dir,
        "hlg.jpg",
        Some("Rec. ITU-R BT.2100 HLG"),
        Some(1),
        None,
        1,
    );
    let signal = scan::detect_signal(&path).unwrap();
    assert_eq!(signal.transfer, HLG);
    assert_eq!(signal.source, hdr::SignalSource::IccProfileName);
    assert!(hdr::is_hdr_signal(&signal));
    assert_eq!(
        hdr::display_mode(Some(&signal), true),
        hdr::DisplayMode::Hdr
    );
}

#[test]
fn pq_icc_profile_is_hdr() {
    let dir = temp_dir("jpeg_pq");
    let path = fixture::write_jpeg_fixture(
        &dir,
        "pq.jpg",
        Some("Rec. ITU-R BT.2100 PQ"),
        Some(1),
        None,
        1,
    );
    let signal = scan::detect_signal(&path).unwrap();
    assert_eq!(signal.transfer, PQ);
    assert!(hdr::is_hdr_signal(&signal));
}

#[test]
fn exif_custom_rendered_3_is_gain_map_hdr() {
    let dir = temp_dir("jpeg_gainmap");
    let path = fixture::write_jpeg_fixture(&dir, "gainmap.jpg", None, Some(1), Some(3), 1);
    let signal = scan::detect_signal(&path).unwrap();
    assert_eq!(signal.source, hdr::SignalSource::ExifCustomRendered);
    assert!(hdr::is_hdr_signal(&signal));
}

/// A file whose pixels are HLG but whose signalling says "unspecified" must not be flagged HDR.
#[test]
fn mislabeled_files_are_not_flagged_hdr() {
    let dir = temp_dir("jpeg_mislabeled");
    // ICC name claims sRGB even though the pixels are HLG.
    let path = fixture::write_jpeg_fixture(
        &dir,
        "mislabeled.jpg",
        Some("sRGB IEC61966-2.1"),
        Some(1),
        None,
        1,
    );
    let signal = scan::detect_signal(&path).unwrap();
    assert_eq!(signal.transfer, SRGB);
    assert!(!hdr::is_hdr_signal(&signal));

    // HEIF with nclx transfer = 2 (unspecified).
    let heic = fixture::write_fixture(&dir, "mislabeled.heic", 2, 2, 0, false);
    let heic_signal = scan::detect_signal(&heic).unwrap();
    assert_eq!(heic_signal.transfer, hdr::TransferFunction::Unspecified);
    assert!(!hdr::is_hdr_signal(&heic_signal));
}

#[test]
fn exif_orientation_is_read() {
    let dir = temp_dir("jpeg_orientation");
    let path = fixture::write_jpeg_fixture(&dir, "rotated.jpg", None, Some(1), None, 6);
    let signal = scan::detect_signal(&path).unwrap();
    assert_eq!(signal.orientation, 6);
}

#[test]
fn scan_folder_mixes_heif_and_jpeg() {
    let dir = temp_dir("jpeg_scan");
    fixture::write_fixture(&dir, "a_heif.heic", 9, 18, 0, true);
    fixture::write_jpeg_fixture(
        &dir,
        "b_jpeg.jpg",
        Some("Rec. ITU-R BT.2100 HLG"),
        Some(1),
        None,
        1,
    );
    fixture::write_jpeg_fixture(&dir, "c_sdr.jpg", None, Some(1), None, 1);

    let entries = scan::scan_folder(&dir);
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].path.file_name().unwrap(), "a_heif.heic");
    assert_eq!(entries[1].path.file_name().unwrap(), "b_jpeg.jpg");
    assert_eq!(entries[2].path.file_name().unwrap(), "c_sdr.jpg");
    assert!(hdr::is_hdr_signal(entries[0].signal.as_ref().unwrap()));
    assert!(hdr::is_hdr_signal(entries[1].signal.as_ref().unwrap()));
    assert!(!hdr::is_hdr_signal(entries[2].signal.as_ref().unwrap()));
}
