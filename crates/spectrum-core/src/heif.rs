//! Minimal ISO-BMFF (HEIF) box reader — only what HDR detection needs.
//!
//! We only walk the box tree; we never decode pixels. `colr` with `colour_type == "nclx"`
//! carries the signalling that decides whether a file is HDR.

/// `(box_type, content)` for each box in an array of boxes.
pub fn boxes(data: &[u8]) -> Vec<([u8; 4], &[u8])> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 8 <= data.len() {
        let size = u32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) as usize;
        let ty = [data[i + 4], data[i + 5], data[i + 6], data[i + 7]];
        let (header_len, total) = if size == 1 {
            if i + 16 > data.len() {
                break;
            }
            let large = u64::from_be_bytes([
                data[i + 8],
                data[i + 9],
                data[i + 10],
                data[i + 11],
                data[i + 12],
                data[i + 13],
                data[i + 14],
                data[i + 15],
            ]) as usize;
            (16, large)
        } else if size == 0 {
            (8, data.len() - i) // extends to end of file
        } else {
            (8, size)
        };
        if total < header_len || i + total > data.len() {
            break;
        }
        out.push((ty, &data[i + header_len..i + total]));
        i += total;
    }
    out
}

/// How many bytes of a container box's content must be skipped before its children start.
///
/// ISO 14496-12 says `meta`/`moov`/`trak`/`mdia`/`minf`/`stbl` are "full boxes" (4-byte
/// version/flags), but real encoders (e.g. FFmpeg-written MP4s) emit them as plain boxes, so we try
/// both. `stsd` additionally carries an entry_count, and a visual sample entry has 42 bytes of fixed
/// fields (6 reserved + data_reference_index + pre_defined/reserved + width/height + resolutions +
/// reserved + frame_count + compressorname + depth + pre_defined). FFmpeg writes a full 32-byte
/// compressorname instead of `string[4]`, which shifts the children by 28 bytes, so 78 is the
/// fallback (verified against `tests/fixtures/hlg_video.mp4`).
pub fn container_skips(ty: &[u8; 4]) -> Option<[usize; 2]> {
    match ty {
        b"stsd" => Some([8, 4]),
        b"avc1" | b"hvc1" | b"av01" | b"mp4v" => Some([42, 78]),
        b"meta" | b"moov" | b"mvex" | b"trak" | b"mdia" | b"minf" | b"stbl" | b"iprp" | b"ipco"
        | b"ipro" => Some([4, 0]),
        _ => None,
    }
}

/// Recursively find the first `nclx` `colr` box content (HEIF property item or MP4/MOV sample entry).
/// ICC-profile `colr` boxes are skipped: they are not nclx signalling.
pub fn find_colr(data: &[u8]) -> Option<&[u8]> {
    for (ty, content) in boxes(data) {
        if ty == *b"colr" {
            if parse_colr(content).is_some() {
                return Some(content);
            }
            continue;
        }
        let Some(skips) = container_skips(&ty) else {
            continue;
        };
        for skip in skips {
            if content.len() > skip {
                if let Some(found) = find_colr(&content[skip..]) {
                    return Some(found);
                }
            }
        }
    }
    None
}

/// Every ICC profile in the box tree, in box order.
///
/// A real HEIC file can carry several: Sony and Apple files put an SDR/gamut profile first and the
/// HDR one (`cicp` or `hdgm`) later, so taking only the first hides the HDR signalling. The caller
/// picks with [`crate::icc::pick_icc`].
pub fn find_icc_profiles(data: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    collect_icc(data, &mut out);
    out
}

fn collect_icc<'a>(data: &'a [u8], out: &mut Vec<&'a [u8]>) {
    for (ty, content) in boxes(data) {
        if ty == *b"colr" {
            if content.len() > 4 && content[..4] == *b"prof" {
                out.push(&content[4..]);
            }
            continue;
        }
        if ty == *b"iccp" {
            if content.len() > 4 {
                out.push(&content[4..]);
            }
            continue;
        }
        let Some(skips) = container_skips(&ty) else {
            continue;
        };
        for skip in skips {
            if content.len() > skip {
                collect_icc(&content[skip..], out);
            }
        }
    }
}

/// The first ICC profile in the box tree. Prefer [`find_icc_profiles`] plus
/// [`crate::icc::pick_icc`], which chooses the profile that actually signals colour.
pub fn find_icc(data: &[u8]) -> Option<&[u8]> {
    for (ty, content) in boxes(data) {
        if ty == *b"colr" {
            if content.len() > 4 && content[..4] == *b"prof" {
                return Some(&content[4..]);
            }
            continue;
        }
        if ty == *b"iccp" {
            // full box: version + flags before the profile
            if content.len() > 4 {
                return Some(&content[4..]);
            }
            continue;
        }
        let Some(skips) = container_skips(&ty) else {
            continue;
        };
        for skip in skips {
            if content.len() > skip {
                if let Some(found) = find_icc(&content[skip..]) {
                    return Some(found);
                }
            }
        }
    }
    None
}

/// Parsed `nclx` colour information.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Nclx {
    pub primaries: u16,
    pub transfer: u16,
    pub matrix: u16,
    pub full_range: bool,
}

/// Parse `colr` content (colour_type + nclx fields).
pub fn parse_colr(content: &[u8]) -> Option<Nclx> {
    if content.len() < 4 || &content[..4] != b"nclx" {
        return None;
    }
    if content.len() < 11 {
        return None;
    }
    let primaries = u16::from_be_bytes([content[4], content[5]]);
    let transfer = u16::from_be_bytes([content[6], content[7]]);
    let matrix = u16::from_be_bytes([content[8], content[9]]);
    let full_range = content[10] & 0x80 != 0;
    Some(Nclx {
        primaries,
        transfer,
        matrix,
        full_range,
    })
}
