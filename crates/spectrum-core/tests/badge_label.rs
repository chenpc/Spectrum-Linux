//! Badge text must name the HDR flavour (gain map / HLG / PQ), matching the macOS reference.

use spectrum_core::hdr;
use spectrum_testkit::fixture;

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("spectrum-core-{tag}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn badge_names_the_hdr_flavour() {
    let dir = temp_dir("badge");

    let gain = fixture::write_jpeg_fixture(&dir, "gainmap.jpg", None, Some(1), Some(3), 1);
    let hlg = fixture::write_jpeg_fixture(
        &dir,
        "hlg.jpg",
        Some("Rec. ITU-R BT.2100 HLG"),
        Some(1),
        None,
        1,
    );
    let pq = fixture::write_jpeg_fixture(
        &dir,
        "pq.jpg",
        Some("Rec. ITU-R BT.2100 PQ"),
        Some(1),
        None,
        1,
    );
    let heif = fixture::write_fixture(&dir, "heif.heic", 9, 18, 0, true);
    let sdr = fixture::write_jpeg_fixture(&dir, "sdr.jpg", None, Some(1), None, 1);

    assert_eq!(
        hdr::badge_label(spectrum_core::scan::detect_signal(&gain).as_ref(), true),
        "HDR"
    );
    assert_eq!(
        hdr::badge_label(spectrum_core::scan::detect_signal(&hlg).as_ref(), true),
        "HLG"
    );
    assert_eq!(
        hdr::badge_label(spectrum_core::scan::detect_signal(&pq).as_ref(), true),
        "PQ"
    );
    assert_eq!(
        hdr::badge_label(spectrum_core::scan::detect_signal(&heif).as_ref(), true),
        "HLG"
    );
    assert_eq!(
        hdr::badge_label(spectrum_core::scan::detect_signal(&sdr).as_ref(), true),
        "SDR"
    );
    assert_eq!(hdr::badge_label(None, true), "SDR");

    // with HDR disabled everything is SDR
    assert_eq!(
        hdr::badge_label(spectrum_core::scan::detect_signal(&hlg).as_ref(), false),
        "SDR"
    );
}

#[test]
fn display_mode_and_badge_agree() {
    let dir = temp_dir("badge_mode");
    let hlg = fixture::write_jpeg_fixture(
        &dir,
        "hlg.jpg",
        Some("Rec. ITU-R BT.2100 HLG"),
        Some(1),
        None,
        1,
    );
    let signal = spectrum_core::scan::detect_signal(&hlg);
    assert_eq!(
        hdr::display_mode(signal.as_ref(), true),
        hdr::DisplayMode::Hdr
    );
    assert_eq!(hdr::badge_label(signal.as_ref(), true), "HLG");
    assert_eq!(
        hdr::display_mode(signal.as_ref(), false),
        hdr::DisplayMode::Sdr
    );
    assert_eq!(hdr::badge_label(signal.as_ref(), false), "SDR");
}
