//! JPEG marker-segment parsing: EXIF (APP1) and ICC profile (APP2).
//!
//! Only the header segments are read — no pixel decoding. This mirrors the macOS reference,
//! which reads `CGImageSourceCopyPropertiesAtIndex` (profile name + EXIF) instead of decoding a
//! thumbnail, because decoding every image for HDR detection was the biggest CPU hotspot when the
//! grid renders many thumbnails.

/// EXIF tag numbers used here.
const TAG_ORIENTATION: u16 = 0x0112;
const TAG_COLORSPACE: u16 = 0x00A0;
const TAG_CUSTOM_RENDERED: u16 = 0x00A4;
const TAG_EXIF_IFD: u16 = 0x8769;
const TAG_MAKE: u16 = 0x010F;
const TAG_MODEL: u16 = 0x0110;
const TAG_EXPOSURE_TIME: u16 = 0x011A;
const TAG_FNUMBER: u16 = 0x829A;
const TAG_ISO: u16 = 0x8827;
const TAG_FOCAL_LENGTH: u16 = 0x9209;
const TAG_DATETIME_ORIGINAL: u16 = 0x9003;

const TYPE_ASCII: u16 = 2;
const TYPE_RATIONAL: u16 = 9;

/// Camera/lens metadata (subset of the macOS reference `EXIFData`: make, model, shutter, aperture,
/// ISO, focal length).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExifData {
    pub make: Option<String>,
    pub model: Option<String>,
    pub iso: Option<u32>,
    /// ExposureTime as (numerator, denominator).
    pub exposure_time: Option<(u32, u32)>,
    /// FNumber as (numerator, denominator).
    pub f_number: Option<(u32, u32)>,
    /// FocalLength as (numerator, denominator).
    pub focal_length: Option<(u32, u32)>,
    /// DateTimeOriginal as written in EXIF, e.g. "2023-11-14 12:34:56".
    pub date_time_original: Option<String>,
}

impl ExifData {
    /// `YYYY-MM-DD` from `DateTimeOriginal`, or None when the string is missing or malformed.
    ///
    /// EXIF writes the date with colons (`2023-11-14 12:34:56`); the import panel groups with the
    /// ISO-style key so it matches `timeline::month_key`.
    pub fn date_key(&self) -> Option<String> {
        let s = self.date_time_original.as_deref()?;
        let b = s.as_bytes();
        if b.len() < 10 || b[4] != b':' || b[7] != b':' {
            return None;
        }
        if ![0, 1, 2, 3, 5, 6, 8, 9]
            .iter()
            .all(|&i| b[i].is_ascii_digit())
        {
            return None;
        }
        Some(format!("{}-{}-{}", &s[0..4], &s[5..7], &s[8..10]))
    }

    /// "1/250s" for sub-second exposures, otherwise seconds (matches the reference `shutterSpeed`).
    pub fn shutter_speed(&self) -> Option<String> {
        let (n, d) = self.exposure_time?;
        if n == 0 {
            return None;
        }
        if n < d {
            Some(format!("1/{}s", d / n))
        } else if n % d == 0 {
            Some(format!("{}s", n / d))
        } else {
            Some(format!("{:.1}s", n as f64 / d as f64))
        }
    }

    pub fn aperture(&self) -> Option<String> {
        let (n, d) = self.f_number?;
        if d == 0 {
            return None;
        }
        let v = n as f64 / d as f64;
        if v.fract() == 0.0 {
            Some(format!("f/{}", v as u32))
        } else {
            Some(format!("f/{:.1}", v))
        }
    }

    pub fn focal_length(&self) -> Option<String> {
        let (n, d) = self.focal_length?;
        if d == 0 {
            return None;
        }
        let v = n as f64 / d as f64;
        if v.fract() == 0.0 {
            Some(format!("{}mm", v as u32))
        } else {
            Some(format!("{:.1}mm", v))
        }
    }

    /// One-line summary for the detail panel, e.g. `Sony ILCE-7RM2 · 1/250s · f/2.8 · 55mm · ISO 100`.
    pub fn summary(&self) -> Option<String> {
        let camera = match (self.make.as_deref(), self.model.as_deref()) {
            (Some(m), Some(md)) => Some(format!("{m} {md}")),
            (Some(m), None) => Some(m.to_string()),
            (None, Some(md)) => Some(md.to_string()),
            (None, None) => None,
        };
        let parts: Vec<String> = [
            camera,
            self.shutter_speed(),
            self.aperture(),
            self.focal_length(),
        ]
        .into_iter()
        .flatten()
        .chain(self.iso.map(|i| format!("ISO {i}")))
        .collect();
        if parts.is_empty() {
            return None;
        }
        Some(parts.join(" · "))
    }
}

/// ISO 21496-1 gain map metadata (JPEG APP2 segment carrying the URN).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GainMap {
    pub urn: String,
    /// HDR headroom baseline as (numerator, denominator).
    pub headroom: Option<(u32, u32)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JpegInfo {
    pub orientation: u16,
    /// EXIF ColorSpace: 0 = unspecified, 1 = sRGB, 0xFFFF = not sRGB.
    pub colorspace: Option<u16>,
    /// EXIF CustomRendered: 3 = "Display" (older iPhone HDR gain-map flag).
    pub custom_rendered: Option<u16>,
    /// ICC profile: description, `cicp` code points and media white point.
    pub icc: crate::icc::IccInfo,
    pub exif: ExifData,
    /// Present when the file carries an ISO 21496-1 gain map.
    pub gain_map: Option<GainMap>,
}

fn u16_be(b: &[u8], at: usize) -> Option<u16> {
    b.get(at..at + 2).map(|s| u16::from_be_bytes([s[0], s[1]]))
}

fn u32_be(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4)
        .map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

/// Read one IFD entry value. `SHORT` (type 3) values with count 1 are stored inline.
fn ifd_value(tiff: &[u8], entry_at: usize, little: bool) -> Option<u32> {
    let typ = if little {
        u16_le(tiff, entry_at + 2)?
    } else {
        u16_be(tiff, entry_at + 2)?
    };
    let count = if little {
        u32_le(tiff, entry_at + 4)?
    } else {
        u32_be(tiff, entry_at + 4)?
    };
    let raw = if little {
        u32_le(tiff, entry_at + 8)?
    } else {
        u32_be(tiff, entry_at + 8)?
    };
    if typ == 3 && count == 1 {
        // inline short: the value occupies the first two bytes of the 4-byte field
        return Some(if little { raw & 0xFFFF } else { raw >> 16 });
    }
    Some(raw)
}

fn u16_le(b: &[u8], at: usize) -> Option<u16> {
    b.get(at..at + 2).map(|s| u16::from_le_bytes([s[0], s[1]]))
}

fn u32_le(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn read_ifd(tiff: &[u8], ifd_at: usize, little: bool, want: &[u16]) -> Option<(u16, u32)> {
    let count = if little {
        u16_le(tiff, ifd_at)?
    } else {
        u16_be(tiff, ifd_at)?
    } as usize;
    for k in 0..count {
        let at = ifd_at + 2 + k * 12;
        let tag = if little {
            u16_le(tiff, at)?
        } else {
            u16_be(tiff, at)?
        };
        if want.contains(&tag) {
            return Some((tag, ifd_value(tiff, at, little)?));
        }
    }
    None
}

fn entry_type(tiff: &[u8], at: usize, little: bool) -> Option<u16> {
    if little {
        u16_le(tiff, at + 2)
    } else {
        u16_be(tiff, at + 2)
    }
}

fn entry_count(tiff: &[u8], at: usize, little: bool) -> Option<u32> {
    if little {
        u32_le(tiff, at + 4)
    } else {
        u32_be(tiff, at + 4)
    }
}

fn find_entry(tiff: &[u8], ifd_at: usize, little: bool, tag: u16) -> Option<usize> {
    let count = if little {
        u16_le(tiff, ifd_at)?
    } else {
        u16_be(tiff, ifd_at)?
    } as usize;
    for k in 0..count {
        let at = ifd_at + 2 + k * 12;
        let t = if little {
            u16_le(tiff, at)?
        } else {
            u16_be(tiff, at)?
        };
        if t == tag {
            return Some(at);
        }
    }
    None
}

/// ASCII value: inline when count <= 4, otherwise an offset into the TIFF.
fn read_string(tiff: &[u8], entry_at: usize, little: bool) -> Option<String> {
    if entry_type(tiff, entry_at, little) != Some(TYPE_ASCII) {
        return None;
    }
    let count = entry_count(tiff, entry_at, little)? as usize;
    let data_at = if count <= 4 {
        entry_at + 8
    } else {
        ifd_value(tiff, entry_at, little)? as usize
    };
    let s = tiff.get(data_at..data_at + count)?;
    let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    Some(String::from_utf8_lossy(&s[..end]).trim().to_string())
}

fn read_rational(tiff: &[u8], entry_at: usize, little: bool) -> Option<(u32, u32)> {
    if entry_type(tiff, entry_at, little) != Some(TYPE_RATIONAL) {
        return None;
    }
    let off = ifd_value(tiff, entry_at, little)? as usize;
    let num = if little {
        u32_le(tiff, off)?
    } else {
        u32_be(tiff, off)?
    };
    let den = if little {
        u32_le(tiff, off + 4)?
    } else {
        u32_be(tiff, off + 4)?
    };
    if den == 0 {
        return None;
    }
    Some((num, den))
}

fn parse_tiff(tiff: &[u8]) -> (u16, Option<u16>, Option<u16>, ExifData) {
    let little = match tiff.get(0..2) {
        Some(b"II") => true,
        Some(b"MM") => false,
        _ => return (1, None, None, ExifData::default()),
    };
    let magic = if little {
        u16_le(tiff, 2)
    } else {
        u16_be(tiff, 2)
    };
    if magic != Some(42) {
        return (1, None, None, ExifData::default());
    }
    let ifd0_raw = if little {
        u32_le(tiff, 4)
    } else {
        u32_be(tiff, 4)
    };
    let Some(ifd0) = ifd0_raw else {
        return (1, None, None, ExifData::default());
    };
    let ifd0 = ifd0 as usize;

    let orientation = read_ifd(tiff, ifd0, little, &[TAG_ORIENTATION]).map(|(_, v)| v as u16);
    let colorspace = read_ifd(tiff, ifd0, little, &[TAG_COLORSPACE]).map(|(_, v)| v as u16);

    // CustomRendered lives in the Exif sub-IFD.
    let exif_ifd = read_ifd(tiff, ifd0, little, &[TAG_EXIF_IFD]).map(|(_, v)| v as usize);
    let custom = exif_ifd
        .and_then(|off| read_ifd(tiff, off, little, &[TAG_CUSTOM_RENDERED]))
        .map(|(_, v)| v as u16);

    let exif = ExifData {
        make: find_entry(tiff, ifd0, little, TAG_MAKE).and_then(|at| read_string(tiff, at, little)),
        model: find_entry(tiff, ifd0, little, TAG_MODEL)
            .and_then(|at| read_string(tiff, at, little)),
        iso: exif_ifd
            .and_then(|off| find_entry(tiff, off, little, TAG_ISO))
            .and_then(|at| ifd_value(tiff, at, little)),
        exposure_time: exif_ifd
            .and_then(|off| find_entry(tiff, off, little, TAG_EXPOSURE_TIME))
            .and_then(|at| read_rational(tiff, at, little)),
        f_number: exif_ifd
            .and_then(|off| find_entry(tiff, off, little, TAG_FNUMBER))
            .and_then(|at| read_rational(tiff, at, little)),
        focal_length: exif_ifd
            .and_then(|off| find_entry(tiff, off, little, TAG_FOCAL_LENGTH))
            .and_then(|at| read_rational(tiff, at, little)),
        date_time_original: exif_ifd
            .and_then(|off| find_entry(tiff, off, little, TAG_DATETIME_ORIGINAL))
            .and_then(|at| read_string(tiff, at, little)),
    };

    (orientation.unwrap_or(1), colorspace, custom, exif)
}

// ICC profile description tag (`desc` = ICC v2/v4 ASCII, `dscm` = ICC v4 Unicode).
//
// APP2 encoders differ in header length (14 or 15 bytes before the profile), so anchor on the
// `acsp` signature (always at profile offset 36) instead of assuming the profile starts at byte 0.
/// ISO 21496-1 APP2 payload: 28-byte NUL-padded URN, version (4), control flags (1), then HDR
/// headroom as four uint32 (baseline num/den, alternate num/den).
fn parse_gain_map(payload: &[u8]) -> Option<GainMap> {
    let urn = String::from_utf8_lossy(&payload[..28])
        .trim_end_matches('\0')
        .to_string();
    if payload.len() < 41 {
        return Some(GainMap {
            urn,
            headroom: None,
        });
    }
    let num = u32_be(payload, 33)?;
    let den = u32_be(payload, 37)?;
    Some(GainMap {
        urn,
        headroom: if den == 0 { None } else { Some((num, den)) },
    })
}

pub fn parse_jpeg(data: &[u8]) -> Option<JpegInfo> {
    if !data.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut i = 2;
    let mut orientation = 1;
    let mut colorspace = None;
    let mut custom = None;
    let mut icc: Vec<u8> = Vec::new();
    let mut exif = ExifData::default();
    let mut gain_map = None;

    while i + 4 <= data.len() {
        if data[i] != 0xFF {
            break;
        }
        let marker = data[i + 1];
        // SOS ends the segment area; the rest is entropy-coded data.
        if marker == 0xDA {
            break;
        }
        // Standalone markers (SOI/EOI/RESET/D0-D7) have no length field.
        if (0xD0..=0xD9).contains(&marker) {
            i += 2;
            continue;
        }
        let len = u16_be(data, i + 2)? as usize;
        let payload = data.get(i + 4..i + 2 + len)?;
        if marker == 0xE1 && payload.starts_with(b"Exif\0\0") {
            let (o, cs, cr, meta) = parse_tiff(&payload[6..]);
            orientation = o;
            colorspace = cs;
            custom = cr;
            exif = meta;
        } else if marker == 0xE2 && payload.starts_with(b"urn:iso:std:iso:ts:21496:-1") {
            gain_map = parse_gain_map(payload);
        } else if marker == 0xE2 && payload.starts_with(b"ICC_PROFILE") {
            // APP2 ICC chunk: "ICC_PROFILE\0" + flags/number/total, then the profile.
            // Header length varies between encoders, so parse_icc() anchors on `acsp`.
            icc.extend_from_slice(&payload[12..]);
        }
        i += 2 + len;
    }

    Some(JpegInfo {
        orientation,
        colorspace,
        custom_rendered: custom,
        icc: crate::icc::parse_icc(&icc).unwrap_or_default(),
        exif,
        gain_map,
    })
}
