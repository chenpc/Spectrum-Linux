//! MP4/MOV `colr` (nclx) reading — the sample-entry path, not the HEIF property path.

use spectrum_core::{hdr, scan};
use spectrum_testkit::fixture;

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("spectrum-core-{tag}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const BT2020: u16 = 9;
const SRGB_CODE: u16 = 13;
const PQ: u16 = 16;
const HLG: u16 = 18;

#[test]
fn hlg_video_is_hdr() {
    let dir = temp_dir("mp4_hlg");
    let path = fixture::write_mp4_fixture(&dir, "hlg.mp4", BT2020, HLG, 0, true);
    let nclx = scan::detect_nclx(&path).expect("colr found in sample entry");
    assert_eq!(nclx.transfer, HLG);
    assert_eq!(nclx.primaries, BT2020);
    assert!(hdr::is_bt2020_primaries(nclx.primaries));
    assert_eq!(
        hdr::display_mode(scan::detect_signal(&path).as_ref(), true),
        hdr::DisplayMode::Hdr
    );
    assert_eq!(
        hdr::badge_label(scan::detect_signal(&path).as_ref(), true),
        "HLG"
    );
}

#[test]
fn pq_video_is_hdr() {
    let dir = temp_dir("mp4_pq");
    let path = fixture::write_mp4_fixture(&dir, "pq.mov", BT2020, PQ, 0, true);
    let signal = scan::detect_signal(&path).unwrap();
    assert_eq!(signal.transfer, hdr::TransferFunction::Pq);
    assert_eq!(hdr::badge_label(Some(&signal), true), "PQ");
}

#[test]
fn srgb_video_is_sdr() {
    let dir = temp_dir("mp4_srgb");
    let path = fixture::write_mp4_fixture(&dir, "srgb.mp4", 1, SRGB_CODE, 0, false);
    let signal = scan::detect_signal(&path).unwrap();
    assert_eq!(signal.transfer, hdr::TransferFunction::Srgb);
    assert!(!hdr::is_hdr_signal(&signal));
    assert_eq!(hdr::badge_label(Some(&signal), true), "SDR");
}

#[test]
fn scan_reports_video_kind_and_signal() {
    let dir = temp_dir("mp4_scan");
    fixture::write_fixture(&dir, "a_photo.heic", BT2020, HLG, 0, true);
    fixture::write_mp4_fixture(&dir, "b_video.mp4", BT2020, HLG, 0, true);

    let entries = scan::scan_folder(&dir);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].kind, scan::MediaKind::Photo);
    assert_eq!(entries[1].kind, scan::MediaKind::Video);
    assert_eq!(entries[1].kind.label(), "video");
    assert!(hdr::is_hdr_signal(entries[1].signal.as_ref().unwrap()));
}
