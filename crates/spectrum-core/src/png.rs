#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

//! PNG header reading, just the colour signalling.
//!
//! PNG has no `colr` box but does have a registered `cICP` chunk (PNG 1.1, ITU-T H.273 codes), and
//! it applies to the image data itself — which is why a `cICP` chunk is an authoritative HDR signal,
//! unlike an ICC profile attached to some item in a HEIF container.

use crate::heif::Nclx;

const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// Walk the chunk list, returning `(type, data)` pairs in file order. Stops at the first chunk that
/// cannot be read: a truncated file is not a licence to guess.
pub fn chunks(data: &[u8]) -> Vec<([u8; 4], &[u8])> {
    if data.len() < SIGNATURE.len() || &data[..SIGNATURE.len()] != SIGNATURE {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut at = SIGNATURE.len();
    while at + 8 <= data.len() {
        let len = u32::from_be_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]) as usize;
        let kind = [data[at + 4], data[at + 5], data[at + 6], data[at + 7]];
        let start = at + 8;
        let end = start + len;
        if end > data.len() {
            break;
        }
        out.push((kind, &data[start..end]));
        // 4 more bytes of CRC after the payload.
        at = end + 4;
    }
    out
}

/// The `cICP` chunk: primaries, transfer, matrix coefficients, and the full-range flag, as H.273
/// codes. Four bytes exactly — a longer or shorter chunk is malformed, so it is not read.
pub fn cicp(data: &[u8]) -> Option<Nclx> {
    let (_, body) = chunks(data)
        .into_iter()
        .find(|(kind, _)| *kind == *b"cICP")?;
    if body.len() != 4 {
        return None;
    }
    Some(Nclx {
        primaries: body[0] as u16,
        transfer: body[1] as u16,
        matrix: body[2] as u16,
        full_range: body[3] & 1 != 0,
    })
}
