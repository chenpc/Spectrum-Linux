//! Real HEIC files carry several ICC profiles: the first is a plain SDR gamut profile and the HDR
//! signalling (`cicp` or `hdgm`) sits in a later one. These tests pin that down.

use spectrum_core::hdr::{is_hdr_signal, SignalSource, TransferFunction};
use spectrum_core::icc::pick_icc;
use spectrum_core::{detect_icc, detect_signal};
use spectrum_testkit::fixture;

const BT2020: u16 = 9;
const SRGB_TRANSFER: u16 = 13;
const UNSPECIFIED: u16 = 2;
// ITU-T H.273 code points, which the ICC `cicp` tag reuses.
const BT2020_10BIT: u8 = 12;
const PQ: u8 = 16;

fn dir(tag: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("spectrum-core-profiles-{tag}"));
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn write(dir: &std::path::Path, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn an_hdr_profile_after_an_sdr_one_is_the_one_that_counts() {
    let sdr = fixture::icc_profile("sRGB IEC61966-2.1", None, None);
    let hdr = fixture::icc_profile("", Some((BT2020_10BIT, PQ, 0, 1)), None);
    let file = fixture::synthetic_heif_with_profiles(BT2020, UNSPECIFIED, 0, true, &[&sdr, &hdr]);
    let path = write(&dir("two"), "sony.heic", &file);

    let signal = detect_signal(&path).expect("a signal");
    assert_eq!(
        signal.transfer,
        TransferFunction::Pq,
        "the `cicp` profile, not the first profile, decides the transfer"
    );
    assert_eq!(signal.source, SignalSource::IccCicp);
    assert!(is_hdr_signal(&signal), "PQ reads as HDR");
    assert!(signal.describe().contains("cicp"), "{}", signal.describe());

    let icc = detect_icc(&path).expect("a profile");
    assert!(
        icc.cicp.is_some(),
        "detect_icc also returns the signalling profile"
    );
    assert_eq!(
        icc.name.as_deref(),
        Some(""),
        "and it is the one with the empty description"
    );
}

#[test]
fn gain_map_metadata_without_a_cicp_tag_still_reads_as_hdr() {
    let gain = fixture::icc_gain_map("Display P3 HD");
    let sdr = fixture::icc_profile("Display P3", None, None);
    let file = fixture::synthetic_heif_with_profiles(BT2020, UNSPECIFIED, 0, true, &[&sdr, &gain]);
    let path = write(&dir("gain"), "apple.heic", &file);

    let signal = detect_signal(&path).expect("a signal");
    assert_eq!(
        signal.source,
        SignalSource::IccGainMap,
        "the `hdgm` tag says there is an HDR representation"
    );
    assert_eq!(
        signal.transfer,
        TransferFunction::Unspecified,
        "gain-map metadata names no transfer function"
    );
    assert!(is_hdr_signal(&signal), "the source alone makes it HDR");
    assert!(signal.describe().contains("hdgm"), "{}", signal.describe());

    let icc = detect_icc(&path).expect("a profile");
    assert!(icc.gain_map, "detect_icc reports the gain map");
}

#[test]
fn pick_icc_orders_by_what_the_profile_actually_says() {
    let sdr = fixture::icc_profile("sRGB", None, None);
    let bright = fixture::icc_profile("bright", None, Some(1000));
    let gain = fixture::icc_gain_map("hd");
    let hdr = fixture::icc_profile("", Some((BT2020_10BIT, PQ, 0, 1)), None);

    let picked = pick_icc(&[&sdr, &bright, &gain, &hdr]).expect("a profile");
    assert!(picked.cicp.is_some(), "cicp beats everything");

    let picked = pick_icc(&[&sdr, &bright, &gain]).expect("a profile");
    assert!(picked.gain_map, "a gain map beats dmnd");

    let picked = pick_icc(&[&sdr, &bright]).expect("a profile");
    assert_eq!(
        picked.media_white_point,
        Some(1000),
        "dmnd beats a description"
    );

    let picked = pick_icc(&[&sdr]).expect("a profile");
    assert_eq!(
        picked.name.as_deref(),
        Some("sRGB"),
        "a plain profile still parses"
    );

    assert!(pick_icc(&[]).is_none(), "no profiles, no signal");
}

#[test]
fn an_sdr_file_stays_sdr() {
    let sdr = fixture::icc_profile("sRGB IEC61966-2.1", None, None);
    let file = fixture::synthetic_heif_with_profiles(11, SRGB_TRANSFER, 0, false, &[&sdr]);
    let path = write(&dir("sdr"), "plain.heic", &file);

    let signal = detect_signal(&path).expect("a signal");
    assert_eq!(signal.transfer, TransferFunction::Srgb);
    assert!(!is_hdr_signal(&signal), "sRGB transfer is not HDR");
}

#[test]
fn bt2020_primary_codes_follow_h273_and_srgb_is_not_bt2020() {
    for code in [9u16, 10, 12, 13] {
        assert!(
            spectrum_core::is_bt2020_primaries(code),
            "code {code} is BT.2020"
        );
    }
    assert!(
        !spectrum_core::is_bt2020_primaries(11),
        "code 11 is sRGB (IEC 61966-2-4)"
    );
}
