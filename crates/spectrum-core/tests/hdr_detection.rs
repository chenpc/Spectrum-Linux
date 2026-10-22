use std::path::PathBuf;

use spectrum_core::{hdr, scan};
use spectrum_testkit::fixture;

// ISO 14496-12 codes used by the fixtures.
const BT2020: u16 = 9;
const BT709: u16 = 1;
const SRGB: u16 = 13;
const PQ: u16 = 16;
const HLG: u16 = 18;

fn temp_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("spectrum-hdr-test-{}-{}", std::process::id(), tag));
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn transfer_codes_match_iso_14496_12() {
    assert_eq!(hdr::transfer_from_code(SRGB), hdr::TransferFunction::Srgb);
    assert_eq!(hdr::transfer_from_code(PQ), hdr::TransferFunction::Pq);
    assert_eq!(hdr::transfer_from_code(HLG), hdr::TransferFunction::Hlg);
    assert_eq!(
        hdr::transfer_from_code(2),
        hdr::TransferFunction::Unspecified
    );
    assert_eq!(hdr::transfer_from_code(4), hdr::TransferFunction::Other(4));
}

#[test]
fn hlg_fixture_is_hdr_with_bt2020_primaries() {
    let dir = temp_dir("hlg");
    let path = fixture::write_fixture(&dir, "hlg.heic", BT2020, HLG, 0, true);
    let nclx = scan::detect_nclx(&path).expect("colr/nclx found");
    assert_eq!(nclx.transfer, HLG);
    assert_eq!(nclx.primaries, BT2020);
    assert!(nclx.full_range);
    assert!(hdr::is_hdr(&nclx));
    assert!(hdr::is_bt2020_primaries(nclx.primaries));
}

#[test]
fn pq_fixture_is_hdr() {
    let dir = temp_dir("pq");
    let path = fixture::write_fixture(&dir, "pq.heic", BT2020, PQ, 0, true);
    let nclx = scan::detect_nclx(&path).unwrap();
    assert!(hdr::is_hdr(&nclx));
}

/// Known limitation, kept as a guard: content carrying HLG signal values but tagged
/// sRGB is reported as SDR, because detection reads signalling only.
#[test]
fn srgb_tagged_content_is_not_hdr() {
    let dir = temp_dir("mislabeled");
    let path = fixture::write_fixture(&dir, "mislabeled.heic", BT709, SRGB, 0, false);
    let nclx = scan::detect_nclx(&path).unwrap();
    assert_eq!(nclx.transfer, SRGB);
    assert!(!hdr::is_hdr(&nclx));
    assert!(!nclx.full_range);
}

#[test]
fn scan_folder_sorts_and_classifies_kinds() {
    let dir = temp_dir("scan");
    fixture::write_fixture(&dir, "b_photo.heic", BT2020, HLG, 0, true);
    fixture::write_fixture(&dir, "a_photo.heic", BT709, SRGB, 0, false);
    std::fs::write(dir.join("notes.txt"), b"not media").unwrap();

    let entries = scan::scan_folder(&dir);
    assert_eq!(entries.len(), 2, "non-media files are skipped");
    assert_eq!(entries[0].path.file_name().unwrap(), "a_photo.heic");
    assert_eq!(entries[1].path.file_name().unwrap(), "b_photo.heic");
    assert_eq!(entries[0].kind, scan::MediaKind::Photo);
    assert!(!hdr::is_hdr_signal(entries[0].signal.as_ref().unwrap()));
    assert!(hdr::is_hdr_signal(entries[1].signal.as_ref().unwrap()));
}

#[test]
fn display_mode_respects_hdr_toggle() {
    let dir = temp_dir("mode");
    let path = fixture::write_fixture(&dir, "hlg.heic", BT2020, HLG, 0, true);
    let signal = scan::detect_signal(&path);
    assert_eq!(
        hdr::display_mode(signal.as_ref(), true),
        hdr::DisplayMode::Hdr
    );
    assert_eq!(
        hdr::display_mode(signal.as_ref(), false),
        hdr::DisplayMode::Sdr
    );
    assert_eq!(hdr::display_mode(None, true), hdr::DisplayMode::Sdr);
}
