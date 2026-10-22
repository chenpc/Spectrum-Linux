//! ICC `cicp` / `dmnd` tags — the paths that do not depend on the profile description text.

use spectrum_core::{hdr, jpeg, scan};
use spectrum_testkit::fixture;

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("spectrum-core-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn cicp_tag_beats_profile_name() {
    let dir = temp_dir("icc_cicp");
    let f = fixture::ExifFixture {
        orientation: 1,
        colorspace: Some(1),
        cicp: Some((9, 18, 0, 0)),
        ..fixture::ExifFixture::default()
    };
    let path = fixture::write_jpeg_fixture_exif(&dir, "cicp.jpg", Some("Display P3"), &f);
    let info = jpeg::parse_jpeg(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        info.icc.cicp,
        Some(spectrum_core::heif::Nclx {
            primaries: 9,
            transfer: 18,
            matrix: 0,
            full_range: false,
        })
    );

    // "Display P3" has no BT.2100 marker, so without the cicp tag this file would be SDR.
    let signal = scan::detect_signal(&path).unwrap();
    assert_eq!(signal.transfer, hdr::TransferFunction::Hlg);
    assert_eq!(signal.source, hdr::SignalSource::IccCicp);
    assert!(hdr::is_hdr_signal(&signal));
    assert_eq!(signal.describe(), "HLG (ICC cicp)");
}

#[test]
fn cicp_pq_is_hdr() {
    let dir = temp_dir("icc_cicp_pq");
    let f = fixture::ExifFixture {
        orientation: 1,
        cicp: Some((9, 16, 0, 1)),
        ..fixture::ExifFixture::default()
    };
    let path = fixture::write_jpeg_fixture_exif(&dir, "pq.jpg", Some("BT2020"), &f);
    let signal = scan::detect_signal(&path).unwrap();
    assert_eq!(signal.transfer, hdr::TransferFunction::Pq);
    assert_eq!(signal.describe(), "PQ (ICC cicp)");
}

#[test]
fn media_white_point_without_cicp_is_pq() {
    let dir = temp_dir("icc_dmnd");
    let f = fixture::ExifFixture {
        orientation: 1,
        dmnd: Some(10000),
        ..fixture::ExifFixture::default()
    };
    let path = fixture::write_jpeg_fixture_exif(&dir, "dmnd.jpg", Some("BT2020"), &f);
    let info = jpeg::parse_jpeg(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(info.icc.cicp, None);
    assert_eq!(info.icc.media_white_point, Some(10000));
    let signal = scan::detect_signal(&path).unwrap();
    assert_eq!(signal.transfer, hdr::TransferFunction::Pq);
    assert_eq!(signal.source, hdr::SignalSource::IccMediaWhitePoint);
}

#[test]
fn sdr_profile_is_not_flagged_hdr() {
    let dir = temp_dir("icc_sdr");
    let f = fixture::ExifFixture {
        orientation: 1,
        colorspace: Some(1),
        dmnd: Some(80),
        ..fixture::ExifFixture::default()
    };
    let path = fixture::write_jpeg_fixture_exif(&dir, "sdr.jpg", Some("sRGB IEC61966-2.1"), &f);
    let signal = scan::detect_signal(&path).unwrap();
    assert_eq!(signal.transfer, hdr::TransferFunction::Srgb);
    assert_eq!(signal.source, hdr::SignalSource::ExifColorSpace);
    assert!(!hdr::is_hdr_signal(&signal));
}

#[test]
fn gain_map_still_beats_cicp() {
    let dir = temp_dir("icc_gainmap_cicp");
    let f = fixture::ExifFixture {
        orientation: 1,
        colorspace: Some(1),
        gain_map: true,
        cicp: Some((9, 18, 0, 0)),
        ..fixture::ExifFixture::default()
    };
    let path = fixture::write_jpeg_fixture_exif(&dir, "gainmap.jpg", Some("Display P3"), &f);
    let signal = scan::detect_signal(&path).unwrap();
    assert_eq!(signal.source, hdr::SignalSource::GainMap);
    assert_eq!(signal.headroom, Some((4, 1)));
}
