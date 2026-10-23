//! Synthetic fixtures — no personal photos in the repo.
//!
//! HEIF fixtures contain only box structure; JPEG fixtures contain only marker segments.
//! That is enough: HDR detection reads colour signalling, not pixels.

use std::path::{Path, PathBuf};

fn write_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn write_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_be_bytes());
}

fn write_u32_le(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn write_u16_le(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn box_bytes(ty: &[u8; 4], content: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    write_u32(&mut out, (8 + content.len()) as u32);
    out.extend_from_slice(ty);
    out.extend_from_slice(content);
    out
}

fn full_box(ty: &[u8; 4], children: &[u8]) -> Vec<u8> {
    let mut content = vec![0, 0, 0, 0]; // version + flags
    content.extend_from_slice(children);
    box_bytes(ty, &content)
}

pub fn nclx_bytes(primaries: u16, transfer: u16, matrix: u16, full_range: bool) -> Vec<u8> {
    let mut content = Vec::new();
    content.extend_from_slice(b"nclx");
    write_u16(&mut content, primaries);
    write_u16(&mut content, transfer);
    write_u16(&mut content, matrix);
    content.push(if full_range { 0x80 } else { 0x00 });
    box_bytes(b"colr", &content)
}

/// `ftyp` + `meta{hdlr, iprp{ipco{colr}}}` (iprp/ipco are plain containers, not full boxes).
pub fn synthetic_heif(primaries: u16, transfer: u16, matrix: u16, full_range: bool) -> Vec<u8> {
    let mut file = box_bytes(b"ftyp", b"heic");
    let hdlr = box_bytes(
        b"hdlr",
        &[
            0, 0, 0, 0, 0, 0, 0, 0, b'p', b'i', b'c', b't', 0, 0, 0, 0, 0, 0, 0, 0,
        ],
    );
    let ipco = box_bytes(
        b"ipco",
        &nclx_bytes(primaries, transfer, matrix, full_range),
    );
    let iprp = box_bytes(b"iprp", &ipco);
    let meta = full_box(b"meta", &[hdlr, iprp].concat());
    file.extend_from_slice(&meta);
    file
}

/// `ftyp` + `meta{hdlr, iprp{ipco{colr(nclx), colr(prof)…}}}` with **several** ICC profiles, in
/// box order. Sony and Apple HEIC files put an SDR gamut profile first and the HDR one later, which
/// is the case a first-profile-wins reader gets wrong.
pub fn synthetic_heif_with_profiles(
    primaries: u16,
    transfer: u16,
    matrix: u16,
    full_range: bool,
    profiles: &[&[u8]],
) -> Vec<u8> {
    let mut file = box_bytes(b"ftyp", b"heic");
    let hdlr = box_bytes(
        b"hdlr",
        &[
            0, 0, 0, 0, 0, 0, 0, 0, b'p', b'i', b'c', b't', 0, 0, 0, 0, 0, 0, 0, 0,
        ],
    );
    let mut ipco = nclx_bytes(primaries, transfer, matrix, full_range);
    for profile in profiles {
        let mut content = b"prof".to_vec();
        content.extend_from_slice(profile);
        ipco.extend_from_slice(&box_bytes(b"colr", &content));
    }
    let iprp = box_bytes(b"iprp", &ipco);
    let meta = full_box(b"meta", &[hdlr, iprp].concat());
    file.extend_from_slice(&meta);
    file
}

/// `ftyp` + `meta{hdlr, iprp{ipco{colr(nclx), colr(prof)}}}` — the profile is the ICC bytes.
pub fn synthetic_heif_with_icc(
    primaries: u16,
    transfer: u16,
    matrix: u16,
    full_range: bool,
    icc: Option<&[u8]>,
) -> Vec<u8> {
    let mut file = box_bytes(b"ftyp", b"heic");
    let hdlr = box_bytes(
        b"hdlr",
        &[
            0, 0, 0, 0, 0, 0, 0, 0, b'p', b'i', b'c', b't', 0, 0, 0, 0, 0, 0, 0, 0,
        ],
    );
    let mut ipco = nclx_bytes(primaries, transfer, matrix, full_range);
    if let Some(profile) = icc {
        let mut content = b"prof".to_vec();
        content.extend_from_slice(profile);
        ipco.extend_from_slice(&box_bytes(b"colr", &content));
    }
    let iprp = box_bytes(b"iprp", &ipco);
    let meta = full_box(b"meta", &[hdlr, iprp].concat());
    file.extend_from_slice(&meta);
    file
}

/// `ftyp` + `meta{hdlr, iprp{ipco{iccp}}}` — profile only, no `nclx` signalling.
pub fn synthetic_heif_iccp(icc: &[u8]) -> Vec<u8> {
    let mut file = box_bytes(b"ftyp", b"heic");
    let hdlr = box_bytes(
        b"hdlr",
        &[
            0, 0, 0, 0, 0, 0, 0, 0, b'p', b'i', b'c', b't', 0, 0, 0, 0, 0, 0, 0, 0,
        ],
    );
    let ipco = full_box(b"iccp", icc);
    let iprp = box_bytes(b"iprp", &ipco);
    let meta = full_box(b"meta", &[hdlr, iprp].concat());
    file.extend_from_slice(&meta);
    file
}

// A HEIF + ICC fixture has seven independent knobs; a struct would only add ceremony for a
// fixture builder that tests call directly.
#[allow(clippy::too_many_arguments)]
pub fn write_heif_fixture_with_icc(
    dir: &Path,
    name: &str,
    primaries: u16,
    transfer: u16,
    matrix: u16,
    full_range: bool,
    icc_name: Option<&str>,
    cicp: Option<(u8, u8, u8, u8)>,
    dmnd: Option<u32>,
) -> PathBuf {
    let profile = icc_profile(icc_name.unwrap_or(""), cicp, dmnd);
    let path = dir.join(name);
    std::fs::write(
        &path,
        synthetic_heif_with_icc(primaries, transfer, matrix, full_range, Some(&profile)),
    )
    .expect("write fixture");
    path
}

/// ICC profile carrying only gain-map metadata: `desc` + `hdgm` whose payload starts with `gmap`,
/// the shape Apple HEIC files use instead of a `cicp` tag. The numbers are the encoder/decoder
/// median-luma fields; the reader only needs the tag's presence.
pub fn icc_gain_map(name: &str) -> Vec<u8> {
    let tags = 2usize;
    let desc_len = 4 + 4 + 4 + 31 + 10;
    let hdgm_len = 4 + 4 + 12; // gmap signature, version, three u32 fields
    let data_at = 132 + tags * 12;
    let desc_at = data_at;
    let hdgm_at = desc_at + desc_len;
    let total = hdgm_at + hdgm_len;

    let mut name_bytes = name.as_bytes()[..name.len().min(31)].to_vec();
    name_bytes.resize(31, 0);

    let mut out = Vec::new();
    write_u32(&mut out, total as u32);
    out.extend_from_slice(&[0; 32]);
    out.extend_from_slice(b"acsp");
    out.extend_from_slice(&[0; 88]);
    write_u32(&mut out, tags as u32);
    for (sig, at, len) in [(*b"desc", desc_at, desc_len), (*b"hdgm", hdgm_at, hdgm_len)] {
        out.extend_from_slice(&sig);
        write_u32(&mut out, at as u32);
        write_u32(&mut out, len as u32);
    }
    out.extend_from_slice(b"desc");
    out.extend_from_slice(&[0; 4]);
    write_u32(&mut out, 31);
    out.extend_from_slice(&name_bytes);
    out.extend_from_slice(&[0; 10]);
    out.extend_from_slice(b"gmap");
    out.extend_from_slice(&[0; 4]); // version
    for value in [230u32, 98, 106] {
        write_u32(&mut out, value);
    }
    out
}

/// ICC profile: 132-byte header (`acsp` at 36, tag count at 128), tag entries at 132, then tag data.
pub fn icc_profile(name: &str, cicp: Option<(u8, u8, u8, u8)>, dmnd: Option<u32>) -> Vec<u8> {
    let tags = 1 + usize::from(cicp.is_some()) + usize::from(dmnd.is_some());
    // descType: signature(4) + reserved(4) + count(4) + 31-byte ASCII + R/G/B + colorant(10)
    let desc_len = 4 + 4 + 4 + 31 + 10;
    let cicp_len = 12; // signature + reserved + 4 code points
    let dmnd_len = 12; // signature + reserved + u32

    let data_at = 132 + tags * 12;
    let desc_at = data_at;
    let cicp_at = desc_at + desc_len;
    let dmnd_at = cicp_at + usize::from(cicp.is_some()) * cicp_len;

    let total = data_at
        + desc_len
        + usize::from(cicp.is_some()) * cicp_len
        + usize::from(dmnd.is_some()) * dmnd_len;

    let mut name_bytes = name.as_bytes()[..name.len().min(31)].to_vec();
    name_bytes.resize(31, 0);

    let mut out = Vec::new();
    write_u32(&mut out, total as u32);
    out.extend_from_slice(&[0; 32]);
    out.extend_from_slice(b"acsp");
    out.extend_from_slice(&[0; 88]);
    write_u32(&mut out, tags as u32);

    let mut entries: Vec<([u8; 4], usize, usize)> = vec![(*b"desc", desc_at, desc_len)];
    if cicp.is_some() {
        entries.push((*b"cicp", cicp_at, cicp_len));
    }
    if dmnd.is_some() {
        entries.push((*b"dmnd", dmnd_at, dmnd_len));
    }
    for (sig, at, len) in &entries {
        out.extend_from_slice(sig);
        write_u32(&mut out, *at as u32);
        write_u32(&mut out, *len as u32);
    }

    out.extend_from_slice(b"desc"); // descType signature
    out.extend_from_slice(&[0; 4]); // reserved
    write_u32(&mut out, 31);
    out.extend_from_slice(&name_bytes);
    out.extend_from_slice(&[0; 10]);

    if let Some((p, t, m, r)) = cicp {
        out.extend_from_slice(b"cicp");
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&[p, t, m, r]);
    }
    if let Some(d) = dmnd {
        out.extend_from_slice(b"dmnd");
        out.extend_from_slice(&[0; 4]);
        write_u32(&mut out, d);
    }
    out
}

/// EXIF block (little-endian TIFF): IFD0 with Orientation/ColorSpace/Make/Model, Exif sub-IFD with
/// CustomRendered/ISO/ExposureTime/FNumber/FocalLength.
#[derive(Debug, Clone, Default)]
pub struct ExifFixture<'a> {
    pub orientation: u16,
    pub colorspace: Option<u16>,
    pub custom_rendered: Option<u16>,
    pub make: Option<&'a str>,
    pub model: Option<&'a str>,
    pub iso: Option<u32>,
    pub exposure: Option<(u32, u32)>,
    pub f_number: Option<(u32, u32)>,
    pub focal_length: Option<(u32, u32)>,
    /// Write an ISO 21496-1 APP2 gain map segment (URN + headroom) when true.
    pub gain_map: bool,
    /// ICC `cicp` tag: (primaries, transfer, matrix, full_range).
    pub cicp: Option<(u8, u8, u8, u8)>,
    /// ICC `dmnd` media neutral depth in cd/m2.
    pub dmnd: Option<u32>,
    /// EXIF DateTimeOriginal string, e.g. "2023-11-14 12:34:56" (tag 0x9003).
    pub date_time_original: Option<&'a str>,
}

fn exif_block(f: &ExifFixture<'_>) -> Vec<u8> {
    // IFD0 entry count decides where the Exif sub-IFD and the data area start. The sub-IFD pointer
    // itself is one IFD0 entry, so it must be counted whenever the sub-IFD has any entry (a
    // date-only EXIF block previously left the pointer uncounted and unreadable).
    let n1 = usize::from(f.custom_rendered.is_some())
        + usize::from(f.iso.is_some())
        + usize::from(f.exposure.is_some())
        + usize::from(f.f_number.is_some())
        + usize::from(f.focal_length.is_some())
        + usize::from(f.date_time_original.is_some());
    let n0 = 1
        + usize::from(f.colorspace.is_some())
        + usize::from(f.make.is_some())
        + usize::from(f.model.is_some())
        + usize::from(n1 > 0);

    let ifd0_at = 8;
    let exif_ifd_at = ifd0_at + 2 + n0 * 12 + 4;
    let data_at = exif_ifd_at + 2 + n1 * 12 + 4;

    let mut make_off = 0;
    let mut model_off = 0;
    let mut next = data_at;
    // ASCII counts include the trailing NUL; anything longer than 4 bytes lives in the data area.
    if let Some(s) = f.make {
        make_off = next;
        next += s.len() + 1;
    }
    if let Some(s) = f.model {
        model_off = next;
        next += s.len() + 1;
    }
    let exp_off = if f.exposure.is_some() { next } else { 0 };
    if f.exposure.is_some() {
        next += 8;
    }
    let fn_off = if f.f_number.is_some() { next } else { 0 };
    if f.f_number.is_some() {
        next += 8;
    }
    let fl_off = if f.focal_length.is_some() { next } else { 0 };
    let date_off = if f.date_time_original.is_some() {
        next
    } else {
        0
    };

    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"II");
    write_u16_le(&mut tiff, 42);
    write_u32_le(&mut tiff, ifd0_at as u32);

    write_u16_le(&mut tiff, n0 as u16);
    // Orientation (SHORT, count 1) — value is inline in the first two bytes.
    write_u16_le(&mut tiff, 0x0112);
    write_u16_le(&mut tiff, 3);
    write_u32_le(&mut tiff, 1);
    write_u16_le(&mut tiff, f.orientation);
    write_u16_le(&mut tiff, 0);
    if let Some(cs) = f.colorspace {
        write_u16_le(&mut tiff, 0x00A0);
        write_u16_le(&mut tiff, 3);
        write_u32_le(&mut tiff, 1);
        write_u16_le(&mut tiff, cs);
        write_u16_le(&mut tiff, 0);
    }
    for (tag, off, s) in [(0x010F, make_off, f.make), (0x0110, model_off, f.model)] {
        if off == 0 {
            continue;
        }
        write_u16_le(&mut tiff, tag);
        write_u16_le(&mut tiff, 2); // ASCII
        write_u32_le(&mut tiff, (s.unwrap_or("").len() + 1) as u32);
        write_u32_le(&mut tiff, off as u32);
    }
    if n1 > 0 {
        write_u16_le(&mut tiff, 0x8769);
        write_u16_le(&mut tiff, 4);
        write_u32_le(&mut tiff, 1);
        write_u32_le(&mut tiff, exif_ifd_at as u32);
    }
    write_u32_le(&mut tiff, 0); // no next IFD

    if n1 > 0 {
        write_u16_le(&mut tiff, n1 as u16);
        if let Some(cr) = f.custom_rendered {
            write_u16_le(&mut tiff, 0x00A4);
            write_u16_le(&mut tiff, 3);
            write_u32_le(&mut tiff, 1);
            write_u16_le(&mut tiff, cr);
            write_u16_le(&mut tiff, 0);
        }
        if let Some(iso) = f.iso {
            write_u16_le(&mut tiff, 0x8827);
            write_u16_le(&mut tiff, 3);
            write_u32_le(&mut tiff, 1);
            write_u32_le(&mut tiff, iso);
        }
        for (tag, off, _r) in [
            (0x011A, exp_off, f.exposure),
            (0x829A, fn_off, f.f_number),
            (0x9209, fl_off, f.focal_length),
        ] {
            if off == 0 {
                continue;
            }
            write_u16_le(&mut tiff, tag);
            write_u16_le(&mut tiff, 9); // RATIONAL
            write_u32_le(&mut tiff, 1);
            write_u32_le(&mut tiff, off as u32);
        }
        if let Some(s) = f.date_time_original {
            write_u16_le(&mut tiff, 0x9003);
            write_u16_le(&mut tiff, 2); // ASCII
            write_u32_le(&mut tiff, (s.len() + 1) as u32);
            write_u32_le(&mut tiff, date_off as u32);
        }
        write_u32_le(&mut tiff, 0); // no next IFD

        for s in [f.make, f.model].into_iter().flatten() {
            tiff.extend_from_slice(s.as_bytes());
            tiff.push(0);
        }
        for r in [f.exposure, f.f_number, f.focal_length]
            .into_iter()
            .flatten()
        {
            write_u32_le(&mut tiff, r.0);
            write_u32_le(&mut tiff, r.1);
        }
        if let Some(s) = f.date_time_original {
            tiff.extend_from_slice(s.as_bytes());
            tiff.push(0);
        }
    }
    tiff
}

fn segment(marker: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = vec![0xFF, marker];
    write_u16(&mut out, (2 + payload.len()) as u16);
    out.extend_from_slice(payload);
    out
}

/// `SOI` + APP1(EXIF) + APP2(ICC) + `EOI`. No scan data — the parser only reads headers.
pub fn synthetic_jpeg(
    icc_name: Option<&str>,
    colorspace: Option<u16>,
    custom_rendered: Option<u16>,
    orientation: u16,
) -> Vec<u8> {
    synthetic_jpeg_exif(
        icc_name,
        &ExifFixture {
            orientation,
            colorspace,
            custom_rendered,
            ..ExifFixture::default()
        },
    )
}

pub fn synthetic_jpeg_exif(icc_name: Option<&str>, f: &ExifFixture<'_>) -> Vec<u8> {
    let mut file = vec![0xFF, 0xD8];
    let has_exif = f.orientation != 1
        || f.date_time_original.is_some()
        || f.colorspace.is_some()
        || f.custom_rendered.is_some()
        || f.make.is_some()
        || f.model.is_some()
        || f.iso.is_some()
        || f.exposure.is_some()
        || f.f_number.is_some()
        || f.focal_length.is_some()
        || f.gain_map;
    if has_exif {
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend_from_slice(&exif_block(f));
        file.extend_from_slice(&segment(0xE1, &app1));
    }
    if f.gain_map {
        let mut app2: Vec<u8> = Vec::new();
        let mut urn = b"urn:iso:std:iso:ts:21496:-1".to_vec();
        while urn.len() < 28 {
            urn.push(0);
        }
        app2.extend_from_slice(&urn);
        write_u32(&mut app2, 1); // min_version + writer_version
        app2.push(0); // control flags
        write_u32(&mut app2, 4); // baseline headroom numerator
        write_u32(&mut app2, 1); // baseline headroom denominator
        write_u32(&mut app2, 1); // alternate headroom numerator
        write_u32(&mut app2, 1); // alternate headroom denominator
        file.extend_from_slice(&segment(0xE2, &app2));
    }
    if icc_name.is_some() || f.cicp.is_some() || f.dmnd.is_some() {
        let mut app2 = b"ICC_PROFILE\0".to_vec();
        app2.extend_from_slice(&[0, 1, 1]); // flags, segment number, segment total
        app2.extend_from_slice(&icc_profile(icc_name.unwrap_or(""), f.cicp, f.dmnd));
        file.extend_from_slice(&segment(0xE2, &app2));
    }
    file.extend_from_slice(&[0xFF, 0xD9]);
    file
}

/// `ftyp` + `moov{trak{mdia{minf{stbl{stsd{avc1{colr}}}}}}}` — the MP4/MOV path used by `find_colr`.
pub fn synthetic_mp4(primaries: u16, transfer: u16, matrix: u16, full_range: bool) -> Vec<u8> {
    let mut file = box_bytes(b"ftyp", b"isom");

    // Visual sample entry: 6 reserved + data_reference_index + pre_defined/reserved + width/height
    // + resolutions + reserved + frame_count + bytes_per_row + bits_per_pixel + pre_defined = 42 bytes
    // before its children.
    let mut entry = Vec::new();
    entry.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
    entry.extend_from_slice(&1u16.to_be_bytes());
    entry.extend_from_slice(&[0, 0, 0, 0]);
    entry.extend_from_slice(&0u16.to_be_bytes());
    entry.extend_from_slice(&0u16.to_be_bytes());
    entry.extend_from_slice(&0u32.to_be_bytes());
    entry.extend_from_slice(&0u32.to_be_bytes());
    entry.extend_from_slice(&0u32.to_be_bytes());
    entry.extend_from_slice(&0u16.to_be_bytes());
    entry.extend_from_slice(&0u32.to_be_bytes());
    entry.extend_from_slice(&0u32.to_be_bytes());
    entry.extend_from_slice(&0u32.to_be_bytes());
    entry.extend_from_slice(&nclx_bytes(primaries, transfer, matrix, full_range));

    let mut stsd_content = vec![0, 0, 0, 0]; // version + flags
    stsd_content.extend_from_slice(&1u32.to_be_bytes()); // entry_count
    stsd_content.extend_from_slice(&box_bytes(b"avc1", &entry));

    let stbl = full_box(b"stbl", &box_bytes(b"stsd", &stsd_content));
    let minf = full_box(b"minf", &stbl);
    let mdia = full_box(b"mdia", &minf);
    let trak = full_box(b"trak", &mdia);

    // mvhd (version 0): version/flags + creation + modification + timescale + duration
    let mut mvhd = vec![0, 0, 0, 0];
    mvhd.extend_from_slice(&0u32.to_be_bytes());
    mvhd.extend_from_slice(&0u32.to_be_bytes());
    mvhd.extend_from_slice(&1000u32.to_be_bytes()); // timescale
    mvhd.extend_from_slice(&12500u32.to_be_bytes()); // duration -> 12.5s
    let moov = full_box(b"moov", &[box_bytes(b"mvhd", &mvhd), trak].concat());
    file.extend_from_slice(&moov);
    file
}

pub fn write_mp4_fixture(
    dir: &Path,
    name: &str,
    primaries: u16,
    transfer: u16,
    matrix: u16,
    full_range: bool,
) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(
        &path,
        synthetic_mp4(primaries, transfer, matrix, full_range),
    )
    .expect("write fixture");
    path
}

pub fn write_fixture(
    dir: &Path,
    name: &str,
    primaries: u16,
    transfer: u16,
    matrix: u16,
    full_range: bool,
) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(
        &path,
        synthetic_heif(primaries, transfer, matrix, full_range),
    )
    .expect("write fixture");
    path
}

pub fn write_jpeg_fixture(
    dir: &Path,
    name: &str,
    icc_name: Option<&str>,
    colorspace: Option<u16>,
    custom_rendered: Option<u16>,
    orientation: u16,
) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(
        &path,
        synthetic_jpeg(icc_name, colorspace, custom_rendered, orientation),
    )
    .expect("write fixture");
    path
}

pub fn write_jpeg_fixture_exif(
    dir: &Path,
    name: &str,
    icc_name: Option<&str>,
    f: &ExifFixture<'_>,
) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, synthetic_jpeg_exif(icc_name, f)).expect("write fixture");
    path
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

fn png_chunk(out: &mut Vec<u8>, typ: &[u8], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(typ);
    out.extend_from_slice(data);
    out.extend_from_slice(&crc32(&[typ, data].concat()).to_be_bytes());
}

/// Minimal valid PNG (solid colour) — glycin can decode it, so it works as a thumbnail fixture.
pub fn synthetic_png(w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
    synthetic_png_inner(w, h, rgb, None)
}

/// PNG with a `cICP` chunk (PNG's registered colour-signalling chunk): primaries, transfer,
/// matrix, full-range flag, in that order, as defined by PNG 1.1 / ITU-T H.273 codes. This is how
/// a fixture can declare HLG or PQ without an ICC profile, which is what the tone-mapping tests
/// need: the pixels stay exactly what was written, and the loader's colour signalling says HDR.
pub fn synthetic_png_with_cicp(
    w: u32,
    h: u32,
    rgb: [u8; 3],
    primaries: u8,
    transfer: u8,
) -> Vec<u8> {
    synthetic_png_inner(w, h, rgb, Some([primaries, transfer, 0, 1]))
}

fn synthetic_png_inner(w: u32, h: u32, rgb: [u8; 3], cicp: Option<[u8; 4]>) -> Vec<u8> {
    let row = rgb.repeat(w as usize);
    let mut raw = Vec::with_capacity(h as usize * (row.len() + 1));
    for _ in 0..h {
        raw.push(0u8); // filter: none
        raw.extend_from_slice(&row);
    }

    // zlib stream of stored (uncompressed) deflate blocks. A stored block carries at most 65 535
    // bytes, so a bigger image needs several: writing the length as `u16` silently truncated
    // anything over that, and glycin then rejected the file with "IDAT does not have enough data".
    let mut zlib = vec![0x78, 0x01];
    let mut rest = raw.as_slice();
    loop {
        let (chunk, is_final) = if rest.len() > 65_535 {
            (&rest[..65_535], false)
        } else {
            (rest, true)
        };
        zlib.push(if is_final { 0x01 } else { 0x00 });
        zlib.extend_from_slice(&(chunk.len() as u16).to_le_bytes());
        zlib.extend_from_slice(&(!(chunk.len() as u16)).to_le_bytes());
        zlib.extend_from_slice(chunk);
        if is_final {
            break;
        }
        rest = &rest[65_535..];
    }
    let mut adler = 1u32;
    for &b in &raw {
        adler = (adler + b as u32) % 65521;
    }
    zlib.extend_from_slice(&adler.to_be_bytes());

    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // bit depth 8, colour type 2 (RGB)
    png_chunk(&mut out, b"IHDR", &ihdr);
    if let Some(cicp) = cicp {
        png_chunk(&mut out, b"cICP", &cicp);
    }
    png_chunk(&mut out, b"IDAT", &zlib);
    png_chunk(&mut out, b"IEND", b"");
    out
}

pub fn write_png_fixture(
    dir: &std::path::Path,
    name: &str,
    w: u32,
    h: u32,
    rgb: [u8; 3],
) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, synthetic_png(w, h, rgb)).unwrap();
    path
}

/// `cICP` PNG on disk: `primaries`/`transfer` are ITU-T H.273 codes (9 = BT.2020, 18 = HLG,
/// 16 = PQ, 13 = sRGB).
pub fn write_png_fixture_cicp(
    dir: &std::path::Path,
    name: &str,
    w: u32,
    h: u32,
    rgb: [u8; 3],
    primaries: u8,
    transfer: u8,
) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(
        &path,
        synthetic_png_with_cicp(w, h, rgb, primaries, transfer),
    )
    .unwrap();
    path
}
