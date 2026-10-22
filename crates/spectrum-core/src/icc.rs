//! ICC profile parsing (the parts that matter for transfer-function detection).
//!
//! The header length differs between encoders, so everything is anchored on the `acsp` signature
//! (offset 36 in every profile checked on this box) and tag offsets are relative to the profile base.

use crate::heif::Nclx;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IccInfo {
    /// Profile description (`desc` / `dscm`).
    pub name: Option<String>,
    /// `cicp` tag (ICC.1:2022 `cicpType`): primaries, transfer, matrix, full-range flag.
    /// This is the authoritative HDR signal when the profile has it.
    pub cicp: Option<Nclx>,
    /// Media neutral depth (`dmnd`) in cd/m2. >= 1000 is the practical HDR signal for profiles
    /// without a `cicp` tag.
    pub media_white_point: Option<u32>,
    /// `hdgm` tag whose payload starts with `gmap`: gain-map metadata, i.e. the file carries an
    /// HDR representation alongside the SDR baseline (Apple HEIC uses this instead of `cicp`).
    pub gain_map: bool,
}

fn u32_be(data: &[u8], at: usize) -> Option<u32> {
    if at + 4 > data.len() {
        return None;
    }
    Some(u32::from_be_bytes([
        data[at],
        data[at + 1],
        data[at + 2],
        data[at + 3],
    ]))
}

pub fn parse_icc(profile: &[u8]) -> Option<IccInfo> {
    let acsp = profile.windows(4).position(|w| w == b"acsp")?;
    if acsp < 36 {
        return None;
    }
    let base = acsp - 36;
    if base + 132 > profile.len() {
        return None;
    }
    let tag_count = u32_be(profile, base + 128)? as usize;
    let mut info = IccInfo::default();

    for k in 0..tag_count {
        let at = base + 132 + k * 12;
        let sig = profile.get(at..at + 4)?;
        let off = base + u32_be(profile, at + 4)? as usize;
        let size = u32_be(profile, at + 8)? as usize;
        let data = profile.get(off..off + size)?;

        match sig {
            b"desc" => {
                // Real profiles start with the type signature, so the count is at 8; some writers
                // omit it and put the count at 0.
                let count_at = usize::from(data.starts_with(b"desc")) * 8;
                let n = u32_be(data, count_at)? as usize;
                let s = data.get(count_at + 4..count_at + 4 + n.min(31))?;
                let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
                info.name = Some(String::from_utf8_lossy(&s[..end]).to_string());
            }
            b"dscm" => {
                let len_at = usize::from(data.starts_with(b"dscm")) * 8;
                let len = u32_be(data, len_at)? as usize;
                let s = data.get(len_at + 4..len_at + 4 + len)?;
                info.name = Some(String::from_utf16_lossy(
                    &s.chunks(2)
                        .map(|c| u16::from_be_bytes([c[0], c[1]]))
                        .collect::<Vec<u16>>(),
                ));
            }
            // cicpType: signature(4) + reserved(4) + primaries, transfer, matrix, full-range (1 each)
            b"cicp" if size >= 12 => {
                info.cicp = Some(Nclx {
                    primaries: data[8] as u16,
                    transfer: data[9] as u16,
                    matrix: data[10] as u16,
                    full_range: data[11] != 0,
                });
            }
            // dataType: signature(4) + reserved(4) + u32 value
            b"dmnd" if size >= 12 => {
                info.media_white_point = u32_be(data, 8);
            }
            // hdrGainMapType: signature(4) + version(4) + `gmap` gain-map metadata
            b"hdgm" if size >= 12 && data.starts_with(b"gmap") => {
                info.gain_map = true;
            }
            _ => {}
        }
    }

    Some(info)
}

/// Choose the profile that says most about the colour signal:
/// a `cicp` tag naming an HDR transfer, then a gain map, then any `cicp` tag, then `dmnd` >=
/// 1000 cd/m2, then a readable description, then whatever parses.
///
/// Files from Sony and Apple carry several profiles in one container. `IMG_5034.HEIC` has four:
/// a Display P3 gamut profile, a short one, a BT.2020 profile whose `cicp` transfer is 8 (BT.709,
/// i.e. SDR), and a fourth whose `cicp` transfer is 16 (PQ) with an `hdgm` gain map. "First profile
/// wins" — or even "first profile with a `cicp` tag" — reads that file as SDR.
pub fn pick_icc(profiles: &[&[u8]]) -> Option<IccInfo> {
    let parsed: Vec<IccInfo> = profiles.iter().copied().filter_map(parse_icc).collect();
    parsed
        .iter()
        .find(|i| i.cicp.as_ref().is_some_and(crate::hdr::is_hdr))
        .or_else(|| parsed.iter().find(|i| i.gain_map))
        .or_else(|| parsed.iter().find(|i| i.cicp.is_some()))
        .or_else(|| {
            parsed
                .iter()
                .find(|i| i.media_white_point.is_some_and(|w| w >= 1000))
        })
        .or_else(|| parsed.iter().find(|i| i.name.is_some()))
        .cloned()
        .or_else(|| parsed.into_iter().next())
}
