//! `ImageSignal::describe()` — the panel text that shows *where* the transfer function came from.

use spectrum_core::hdr::{ImageSignal, SignalSource, TransferFunction};

fn sig(
    source: SignalSource,
    transfer: TransferFunction,
    headroom: Option<(u32, u32)>,
) -> ImageSignal {
    ImageSignal {
        transfer,
        source,
        orientation: 1,
        exif: None,
        headroom,
    }
}

#[test]
fn describe_names_the_source() {
    let hlg = TransferFunction::Hlg;
    assert_eq!(sig(SignalSource::Nclx, hlg, None).describe(), "HLG (nclx)");
    assert_eq!(
        sig(SignalSource::IccProfileName, hlg, None).describe(),
        "HLG (ICC name)"
    );
    assert_eq!(
        sig(SignalSource::ExifColorSpace, TransferFunction::Srgb, None).describe(),
        "sRGB (EXIF)"
    );
    assert_eq!(
        sig(
            SignalSource::ExifMetadataOnly,
            TransferFunction::Unspecified,
            None
        )
        .describe(),
        "EXIF metadata only"
    );
    assert_eq!(
        sig(
            SignalSource::ExifCustomRendered,
            TransferFunction::Unspecified,
            None
        )
        .describe(),
        "gain map (EXIF CustomRendered)"
    );
}

#[test]
fn gain_map_describe_includes_headroom() {
    let gm = SignalSource::GainMap;
    assert_eq!(
        sig(gm, TransferFunction::Unspecified, Some((4, 1))).describe(),
        "gain map 4.0× (ISO 21496-1)"
    );
    assert_eq!(
        sig(gm, TransferFunction::Unspecified, Some((3, 2))).describe(),
        "gain map 1.5× (ISO 21496-1)"
    );
    // no headroom (or a zero denominator) must not divide by zero
    assert_eq!(
        sig(gm, TransferFunction::Unspecified, None).describe(),
        "gain map (ISO 21496-1)"
    );
    assert_eq!(
        sig(gm, TransferFunction::Unspecified, Some((0, 1))).describe(),
        "gain map (ISO 21496-1)"
    );
    assert_eq!(
        sig(gm, TransferFunction::Unspecified, Some((4, 0))).describe(),
        "gain map (ISO 21496-1)"
    );
}

/// The mislabel case is exactly what the describe line is for: the file says sRGB in the ICC
/// description even though the pixels are HLG.
#[test]
fn mislabeled_file_shows_the_icc_name_not_nclx() {
    let dir = std::env::temp_dir().join("spectrum-core-describe");
    std::fs::create_dir_all(&dir).unwrap();
    let path = spectrum_testkit::fixture::write_jpeg_fixture(
        &dir,
        "mislabeled.jpg",
        Some("sRGB IEC61966-2.1"),
        Some(1),
        None,
        1,
    );
    let signal = spectrum_core::scan::detect_signal(&path).unwrap();
    // The ICC name is not a BT.2100 marker, so the decision actually came from EXIF ColorSpace.
    assert_eq!(signal.describe(), "sRGB (EXIF)");
    assert!(!spectrum_core::is_hdr_signal(&signal));

    // A file whose ICC name *is* a BT.2100 marker reports that as the source.
    let hdr = spectrum_testkit::fixture::write_jpeg_fixture(
        &dir,
        "hlg_icc.jpg",
        Some("Rec. ITU-R BT.2100 HLG"),
        Some(1),
        None,
        1,
    );
    let hdr_signal = spectrum_core::scan::detect_signal(&hdr).unwrap();
    assert_eq!(hdr_signal.describe(), "HLG (ICC name)");
    assert!(spectrum_core::is_hdr_signal(&hdr_signal));
}
