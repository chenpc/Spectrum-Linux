//! HDR signalling: transfer function codes from ISO/IEC 14496-12 (`nclx`).
//!
//! Codes verified against ISO 14496-12 / codecs.wiki:
//!   13 = IEC 61966-2-1 (sRGB), 16 = ITU-R BT.2100 PQ, 18 = ITU-R BT.2100 HLG.

use crate::heif::Nclx;
use crate::jpeg::JpegInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferFunction {
    Srgb,
    Pq,
    Hlg,
    Unspecified,
    Other(u16),
}

pub fn transfer_from_code(code: u16) -> TransferFunction {
    match code {
        13 => TransferFunction::Srgb,
        16 => TransferFunction::Pq,
        18 => TransferFunction::Hlg,
        2 => TransferFunction::Unspecified,
        other => TransferFunction::Other(other),
    }
}

/// BT.2020 primaries, per the ITU-T H.273 colour-primary code points: 9 (BT.2020 constant
/// luminance), 10 (non-constant luminance), 12 (10-bit), 13 (12-bit). Code 11 is sRGB
/// (IEC 61966-2-4), so it is *not* BT.2020 — the ICC `cicp` tag uses these same code points.
pub fn is_bt2020_primaries(code: u16) -> bool {
    matches!(code, 9 | 10 | 12 | 13)
}

pub fn is_hdr(nclx: &Nclx) -> bool {
    matches!(
        transfer_from_code(nclx.transfer),
        TransferFunction::Pq | TransferFunction::Hlg
    )
}

/// Where the transfer function came from. The macOS reference checks these in this order:
/// gain-map auxiliary data, EXIF CustomRendered == 3, ICC profile name, then the colourspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalSource {
    Nclx,
    IccProfileName,
    ExifCustomRendered,
    ExifColorSpace,
    /// ISO 21496-1 gain map (APP2 URN segment) — the reference's first choice.
    GainMap,
    /// EXIF was present but carried no colour signalling (only orientation/metadata).
    ExifMetadataOnly,
    /// ICC `cicp` tag (ICC.1:2022) — explicit code points, beats the profile description.
    IccCicp,
    /// ICC `dmnd` media white point >= 1000 cd/m2 (no `cicp` tag present).
    IccMediaWhitePoint,
    /// ICC `hdgm`/`gmap` gain-map metadata (Apple HEIC HDR without a `cicp` tag).
    IccGainMap,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageSignal {
    pub transfer: TransferFunction,
    pub source: SignalSource,
    /// EXIF orientation (1 = normal). 0 when the file has no EXIF.
    pub orientation: u16,
    /// Camera/lens metadata (JPEG only; None for HEIF boxes).
    pub exif: Option<crate::jpeg::ExifData>,
    /// Gain-map headroom (num/den) when the signal came from an ISO 21496-1 gain map.
    pub headroom: Option<(u32, u32)>,
}

impl TransferFunction {
    pub fn name(&self) -> &'static str {
        match self {
            TransferFunction::Srgb => "sRGB",
            TransferFunction::Pq => "PQ",
            TransferFunction::Hlg => "HLG",
            TransferFunction::Unspecified => "unspecified",
            TransferFunction::Other(_) => "other",
        }
    }
}

impl ImageSignal {
    /// Human-readable "transfer function (where it came from)", e.g. `HLG (ICC name)` or
    /// `gain map 4.0× (ISO 21496-1)`. This is what tells a mislabeled file apart from a signalled one.
    pub fn describe(&self) -> String {
        let t = match self.transfer {
            TransferFunction::Other(code) => format!("code {code}"),
            other => other.name().to_string(),
        };
        match self.source {
            SignalSource::Nclx => format!("{t} (nclx)"),
            SignalSource::IccProfileName => format!("{t} (ICC name)"),
            SignalSource::IccCicp => format!("{t} (ICC cicp)"),
            SignalSource::IccMediaWhitePoint => format!("{t} (ICC media white point)"),
            SignalSource::IccGainMap => "gain map (ICC hdgm)".to_string(),
            SignalSource::ExifColorSpace => format!("{t} (EXIF)"),
            SignalSource::ExifMetadataOnly => "EXIF metadata only".to_string(),
            SignalSource::ExifCustomRendered => "gain map (EXIF CustomRendered)".to_string(),
            SignalSource::GainMap => match self.headroom {
                Some((n, d)) if n != 0 && d != 0 => {
                    format!("gain map {:.1}× (ISO 21496-1)", n as f64 / d as f64)
                }
                _ => "gain map (ISO 21496-1)".to_string(),
            },
        }
    }
}

/// ICC profile descriptions for BT.2100 look like "Rec. ITU-R BT.2100 HLG" / "... PQ".
/// The macOS reference matches "2100" or "HLG"; PQ is separated here so the badge can tell them apart.
pub fn transfer_from_profile_name(name: &str) -> Option<TransferFunction> {
    let up = name.to_ascii_uppercase();
    if up.contains("HLG") {
        Some(TransferFunction::Hlg)
    } else if up.contains("PQ") {
        Some(TransferFunction::Pq)
    } else if up.contains("2100") {
        Some(TransferFunction::Hlg)
    } else {
        None
    }
}

/// EXIF ColorSpace values: 0 = unspecified, 1 = sRGB, 0xFFFF = not sRGB.
pub fn transfer_from_exif_colorspace(value: u16) -> TransferFunction {
    match value {
        1 => TransferFunction::Srgb,
        0 => TransferFunction::Unspecified,
        other => TransferFunction::Other(other),
    }
}

pub fn is_hdr_signal(signal: &ImageSignal) -> bool {
    matches!(
        signal.transfer,
        TransferFunction::Pq | TransferFunction::Hlg
    ) || matches!(
        signal.source,
        SignalSource::ExifCustomRendered | SignalSource::GainMap | SignalSource::IccGainMap
    )
}

/// ICC-based transfer + source, in priority order: `cicp` tag → `hdgm` gain-map metadata → `dmnd`
/// media white point (>= 1000 cd/m2) → profile description.
///
/// A gain map without a `cicp` tag says "this file has an HDR representation" without naming a
/// transfer function, so the transfer stays unspecified and `is_hdr_signal` reads the source.
pub fn icc_signal(icc: &crate::icc::IccInfo) -> Option<(TransferFunction, SignalSource)> {
    if let Some(c) = &icc.cicp {
        return Some((transfer_from_code(c.transfer), SignalSource::IccCicp));
    }
    if icc.gain_map {
        return Some((TransferFunction::Unspecified, SignalSource::IccGainMap));
    }
    if let Some(w) = &icc.media_white_point {
        if *w >= 1000 {
            return Some((TransferFunction::Pq, SignalSource::IccMediaWhitePoint));
        }
    }
    if let Some(name) = &icc.name {
        if let Some(t) = transfer_from_profile_name(name) {
            return Some((t, SignalSource::IccProfileName));
        }
    }
    None
}

/// Build the signal for a HEIF/HEIC or MP4/MOV file from the box tree.
///
/// Priority: `nclx` code points (explicit) → ICC `cicp` → `dmnd` → ICC name. An `nclx` with
/// transfer 2 ("unspecified") carries no transfer information, so the ICC profile decides — that is
/// the case a profile-only HEIF lands in.
pub fn signal_for_heif(
    nclx: Option<&Nclx>,
    icc: Option<&crate::icc::IccInfo>,
) -> Option<ImageSignal> {
    if let Some(n) = nclx {
        let t = transfer_from_code(n.transfer);
        if t != TransferFunction::Unspecified {
            return Some(ImageSignal {
                transfer: t,
                source: SignalSource::Nclx,
                orientation: 0,
                exif: None,
                headroom: None,
            });
        }
    }
    if let Some(i) = icc {
        if let Some((t, source)) = icc_signal(i) {
            return Some(ImageSignal {
                transfer: t,
                source,
                orientation: 0,
                exif: None,
                headroom: None,
            });
        }
    }
    nclx.map(|n| ImageSignal {
        transfer: transfer_from_code(n.transfer),
        source: SignalSource::Nclx,
        orientation: 0,
        exif: None,
        headroom: None,
    })
}

/// Build the signal for a JPEG from EXIF + ICC, in the same priority order as the reference.
pub fn signal_for_jpeg(info: &JpegInfo) -> ImageSignal {
    // 1. gain map (APP2 URN) beats everything else, as in the reference.
    if info.gain_map.is_some() {
        return ImageSignal {
            transfer: TransferFunction::Unspecified,
            source: SignalSource::GainMap,
            orientation: info.orientation,
            exif: Some(info.exif.clone()),
            headroom: info.gain_map.as_ref().and_then(|g| g.headroom),
        };
    }
    // 2. an ICC `cicp` tag is explicit code points, so it beats the profile description.
    if let Some(cicp) = &info.icc.cicp {
        return ImageSignal {
            transfer: transfer_from_code(cicp.transfer),
            source: SignalSource::IccCicp,
            orientation: info.orientation,
            exif: Some(info.exif.clone()),
            headroom: None,
        };
    }
    if info.custom_rendered == Some(3) {
        return ImageSignal {
            transfer: TransferFunction::Unspecified,
            source: SignalSource::ExifCustomRendered,
            orientation: info.orientation,
            exif: Some(info.exif.clone()),
            headroom: None,
        };
    }
    // 3. no `cicp` tag: a media white point >= 1000 cd/m2 is the practical HDR signal.
    if let Some(w) = info.icc.media_white_point {
        if w >= 1000 {
            return ImageSignal {
                transfer: TransferFunction::Pq,
                source: SignalSource::IccMediaWhitePoint,
                orientation: info.orientation,
                exif: Some(info.exif.clone()),
                headroom: None,
            };
        }
    }
    if let Some(name) = &info.icc.name {
        if let Some(transfer) = transfer_from_profile_name(name) {
            return ImageSignal {
                transfer,
                source: SignalSource::IccProfileName,
                orientation: info.orientation,
                exif: Some(info.exif.clone()),
                headroom: None,
            };
        }
    }
    ImageSignal {
        transfer: info
            .colorspace
            .map(transfer_from_exif_colorspace)
            .unwrap_or(TransferFunction::Unspecified),
        source: if info.colorspace.is_some() {
            SignalSource::ExifColorSpace
        } else {
            SignalSource::ExifMetadataOnly
        },
        orientation: info.orientation,
        exif: Some(info.exif.clone()),
        headroom: None,
    }
}

/// Display mode the viewer should use for this file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayMode {
    /// Render with native HDR headroom.
    Hdr,
    /// Tone-map to SDR.
    Sdr,
}

pub fn display_mode(signal: Option<&ImageSignal>, hdr_enabled: bool) -> DisplayMode {
    if hdr_enabled && signal.map(is_hdr_signal).unwrap_or(false) {
        DisplayMode::Hdr
    } else {
        DisplayMode::Sdr
    }
}

impl DisplayMode {
    /// Text shown in the detail-view badge.
    pub fn label(&self) -> &'static str {
        match self {
            DisplayMode::Hdr => "HDR",
            DisplayMode::Sdr => "SDR",
        }
    }
}

/// Badge text that distinguishes the HDR flavour, matching the macOS reference `HDRFormat`
/// labels: gain map -> "HDR", HLG -> "HLG", PQ -> "PQ".
pub fn badge_label(signal: Option<&ImageSignal>, hdr_enabled: bool) -> &'static str {
    if !hdr_enabled {
        return "SDR";
    }
    let Some(s) = signal else {
        return "SDR";
    };
    if matches!(
        s.source,
        SignalSource::ExifCustomRendered | SignalSource::GainMap
    ) {
        return "HDR";
    }
    match s.transfer {
        TransferFunction::Hlg => "HLG",
        TransferFunction::Pq => "PQ",
        _ => "SDR",
    }
}
