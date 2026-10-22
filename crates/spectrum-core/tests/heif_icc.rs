use std::path::PathBuf;

use spectrum_core::{hdr, heif, icc, scan};
use spectrum_testkit::fixture;

const BT2020: u16 = 9;
const SRGB: u16 = 13;
const PQ: u16 = 16;
const HLG: u16 = 18;
const UNSPECIFIED: u16 = 2;

fn temp_dir(tag: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("spectrum-heif-icc-{}-{}", std::process::id(), tag));
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// A HEIF whose `nclx` says "unspecified" but whose ICC profile has a `cicp` tag: the profile is the
/// only transfer signalling, so it must win.
#[test]
fn unspecified_nclx_falls_back_to_the_profile_cicp_tag() {
    let dir = temp_dir("cicp");
    let path = fixture::write_heif_fixture_with_icc(
        &dir,
        "cicp.heic",
        2,
        UNSPECIFIED,
        0,
        false,
        Some("Display P3"),
        Some((9, HLG as u8, 0, 0)),
        None,
    );
    let signal = scan::detect_signal(&path).expect("signal");
    assert_eq!(signal.transfer, hdr::TransferFunction::Hlg);
    assert_eq!(signal.source, hdr::SignalSource::IccCicp);
    assert_eq!(signal.describe(), "HLG (ICC cicp)");
    assert!(hdr::is_hdr_signal(&signal));
}

/// When `nclx` names a transfer function it is explicit code points, so it beats the profile.
#[test]
fn nclx_beats_the_profile_when_it_names_a_transfer() {
    let dir = temp_dir("nclx-wins");
    let path = fixture::write_heif_fixture_with_icc(
        &dir,
        "both.heic",
        BT2020,
        HLG,
        0,
        true,
        Some("Display P3"),
        Some((9, PQ as u8, 0, 1)),
        None,
    );
    let signal = scan::detect_signal(&path).expect("signal");
    assert_eq!(signal.source, hdr::SignalSource::Nclx);
    assert_eq!(signal.transfer, hdr::TransferFunction::Hlg);
    assert_eq!(signal.describe(), "HLG (nclx)");
}

/// No `cicp` tag: the profile description is the fallback, as it is for JPEG.
#[test]
fn profile_name_is_the_fallback_without_a_cicp_tag() {
    let dir = temp_dir("name");
    let path = fixture::write_heif_fixture_with_icc(
        &dir,
        "name.heic",
        2,
        UNSPECIFIED,
        0,
        false,
        Some("Rec. ITU-R BT.2100 PQ"),
        None,
        None,
    );
    let signal = scan::detect_signal(&path).expect("signal");
    assert_eq!(signal.transfer, hdr::TransferFunction::Pq);
    assert_eq!(signal.source, hdr::SignalSource::IccProfileName);
    assert_eq!(signal.describe(), "PQ (ICC name)");
}

/// `dmnd` media white point >= 1000 cd/m2 is the practical HDR signal when there is no `cicp`.
#[test]
fn media_white_point_without_cicp_is_pq() {
    let dir = temp_dir("dmnd");
    let path = fixture::write_heif_fixture_with_icc(
        &dir,
        "dmnd.heic",
        2,
        UNSPECIFIED,
        0,
        false,
        Some("Display P3"),
        None,
        Some(10000),
    );
    let signal = scan::detect_signal(&path).expect("signal");
    assert_eq!(signal.transfer, hdr::TransferFunction::Pq);
    assert_eq!(signal.source, hdr::SignalSource::IccMediaWhitePoint);
    assert_eq!(signal.describe(), "PQ (ICC media white point)");
}

/// A profile-only HEIF (`iccp` full box, no `nclx` at all) must still be read.
#[test]
fn iccp_box_is_found_without_an_nclx() {
    let dir = temp_dir("iccp");
    let profile = fixture::icc_profile("Display P3", Some((9, HLG as u8, 0, 0)), None);
    let path = dir.join("profile_only.heic");
    std::fs::write(&path, fixture::synthetic_heif_iccp(&profile)).unwrap();

    let data = std::fs::read(&path).unwrap();
    assert!(heif::find_colr(&data).is_none(), "no nclx box should exist");
    let found = heif::find_icc(&data).expect("iccp profile found");
    let info = icc::parse_icc(found).expect("profile parsed");
    assert_eq!(info.name.as_deref(), Some("Display P3"));
    assert_eq!(info.cicp.map(|c| c.transfer), Some(HLG));

    let signal = scan::detect_signal(&path).expect("signal");
    assert_eq!(signal.source, hdr::SignalSource::IccCicp);
    assert_eq!(signal.transfer, hdr::TransferFunction::Hlg);
}

/// An SDR profile must not be flagged HDR when `nclx` says sRGB.
#[test]
fn sdr_profile_does_not_flag_hdr() {
    let dir = temp_dir("sdr");
    let path = fixture::write_heif_fixture_with_icc(
        &dir,
        "sdr.heic",
        1,
        SRGB,
        0,
        false,
        Some("sRGB IEC61966-2.1"),
        None,
        Some(80),
    );
    let signal = scan::detect_signal(&path).expect("signal");
    assert_eq!(signal.source, hdr::SignalSource::Nclx);
    assert_eq!(signal.transfer, hdr::TransferFunction::Srgb);
    assert!(!hdr::is_hdr_signal(&signal));
}
