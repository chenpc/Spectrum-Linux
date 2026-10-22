//! Parser regression tests against the real fixtures copied from the macOS reference
//! (`SpectrumTests/Fixtures/`). These are encoder-produced files, not our synthetic writers, so they
//! catch layout assumptions the synthetic fixtures cannot.

use std::path::PathBuf;

use spectrum_core::{hdr, jpeg, scan};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

#[test]
fn real_heif_hlg_is_hdr() {
    let p = fixtures().join("hlg_synthetic_patches.heic");
    let bytes = std::fs::read(&p).unwrap();
    let nclx =
        spectrum_core::heif::parse_colr(spectrum_core::heif::find_colr(&bytes).unwrap()).unwrap();
    assert_eq!(nclx.primaries, 9);
    assert_eq!(nclx.transfer, 18); // BT.2100 HLG
    assert_eq!(nclx.matrix, 1);
    assert!(hdr::is_hdr(&nclx));
    assert_eq!(
        hdr::badge_label(scan::detect_signal(&p).as_ref(), true),
        "HLG"
    );
}

#[test]
fn real_mislabeled_heif_is_not_flagged_hdr() {
    for name in ["hlg_mislabeled_srgb.heic", "slog3_mislabeled_srgb.heic"] {
        let p = fixtures().join(name);
        let signal = scan::detect_signal(&p).unwrap();
        assert_eq!(
            signal.transfer,
            hdr::TransferFunction::Unspecified,
            "{name}"
        );
        assert!(!hdr::is_hdr_signal(&signal), "{name}");
        assert_eq!(hdr::badge_label(Some(&signal), true), "SDR", "{name}");
    }
}

#[test]
fn real_ffmpeg_mp4_hlg_is_hdr() {
    // FFmpeg writes moov/trak/mdia/minf/stbl as plain boxes and a 32-byte compressorname, so the
    // sample-entry children start at 78, not the spec's 42.
    let p = fixtures().join("hlg_video.mp4");
    let bytes = std::fs::read(&p).unwrap();
    let nclx =
        spectrum_core::heif::parse_colr(spectrum_core::heif::find_colr(&bytes).unwrap()).unwrap();
    assert_eq!(nclx.transfer, 18);
    assert_eq!(nclx.primaries, 9);
    assert_eq!(
        hdr::badge_label(scan::detect_signal(&p).as_ref(), true),
        "HLG"
    );
}

#[test]
fn real_sdr_video_has_no_signal() {
    let p = fixtures().join("sdr_video.mp4");
    assert_eq!(
        spectrum_core::heif::find_colr(&std::fs::read(&p).unwrap()),
        None
    );
    assert_eq!(scan::detect_signal(&p), None);
    assert_eq!(hdr::badge_label(None, true), "SDR");
}

#[test]
fn real_jpeg_without_colour_signalling_is_sdr() {
    let p = fixtures().join("sdr_photo.jpg");
    let info = jpeg::parse_jpeg(&std::fs::read(&p).unwrap()).unwrap();
    assert_eq!(info.icc.name, None);
    assert_eq!(info.colorspace, None);
    assert_eq!(info.custom_rendered, None);
    let signal = scan::detect_signal(&p).unwrap();
    assert_eq!(signal.source, hdr::SignalSource::ExifMetadataOnly);
    assert!(!hdr::is_hdr_signal(&signal));
    assert_eq!(hdr::badge_label(Some(&signal), true), "SDR");
}

#[test]
fn real_fixtures_scan_is_sorted_and_complete() {
    let entries = scan::scan_folder(&fixtures());
    assert_eq!(entries.len(), 6);
    let names: Vec<String> = entries.iter().map(|e| e.name()).collect();
    assert_eq!(
        names,
        [
            "hlg_mislabeled_srgb.heic",
            "hlg_synthetic_patches.heic",
            "hlg_video.mp4",
            "sdr_photo.jpg",
            "sdr_video.mp4",
            "slog3_mislabeled_srgb.heic",
        ]
    );
    assert_eq!(entries[2].kind, scan::MediaKind::Video);
    assert_eq!(entries[4].kind, scan::MediaKind::Video);
    assert!(hdr::is_hdr_signal(entries[2].signal.as_ref().unwrap()));
    assert!(entries[4].signal.is_none());
}
