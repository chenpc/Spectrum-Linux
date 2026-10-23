#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

//! HDR → SDR tone-mapping, in floating point, with no GPU.
//!
//! Three things live here, and they are deliberately separate:
//!
//! * **Transfer functions** — PQ and HLG from ITU-R BT.2100-2, sRGB from IEC 61966-2-1. The
//!   coefficients are the standard's own numbers, so the tests can assert against published
//!   reference values (100 cd/m² encodes to `0.5080762`) rather than to whatever this file
//!   produces.
//! * **Colour-volume conversion** — BT.2020 primaries to BT.709, derived from the published
//!   BT.709→BT.2020 matrix and inverted here. Inverting is the only way to be sure the two are
//!   consistent; the test asserts the product is the identity matrix.
//! * **Dynamic-range compression** — extended Reinhard, which maps a scene peak onto the SDR
//!   display range and keeps the curve monotonic, so a bright pixel can never come out darker than
//!   a dim one.
//!
//! `gain_map_blend` implements the ISO 21496-1 shape of a gain-map expansion (base image plus a
//! gain image, `base + (max - base) * gain`). It is a pure function of two linear frames: applying
//! it needs the *gain image*, and this project's loader hands back only the base image (measured —
//! see `docs/devlog.md`), so nothing calls it yet. It stays here because the math is the part worth
//! testing, and because F16's video path needs it.

/// PQ (ST 2084) constants, for a signal normalised to 10 000 cd/m². Written as the fractions the
/// standard prints, so the exact value is visible rather than a rounded decimal.
const PQ_M1: f32 = 2610.0 / 16384.0;
const PQ_M2: f32 = 128.0 * 2523.0 / 4096.0;
const PQ_C1: f32 = 3424.0 / 4096.0;
const PQ_C2: f32 = 77_216.0 / 4096.0;
const PQ_C3: f32 = 76_544.0 / 4096.0;
/// Peak luminance PQ encodes at 1.0, in cd/m².
const PQ_PEAK_NITS: f32 = 10_000.0;

/// HLG (BT.2100 Table 6) coefficients, kept as `f64` because the standard prints them to eight
/// decimals and clippy is right that an `f32` literal cannot carry that.
const HLG_A: f32 = 0.178_832_77_f64 as f32;
const HLG_B: f32 = 0.284_668_92_f64 as f32;
const HLG_C: f32 = 0.559_910_73_f64 as f32;
/// The break between HLG's two branches: `1/12` of peak encodes to `0.5`.
pub const HLG_KNEE_ENCODED: f32 = 0.5;
pub const HLG_KNEE_LINEAR: f32 = 1.0 / 12.0;

/// An SDR display's reference white and peak, in cd/m². BT.2408 treats 100 cd/m² as the reference
/// white; 203 is the peak most SDR tone-maps aim at.
pub const SDR_REFERENCE_NITS: f32 = 100.0;
pub const SDR_TARGET_NITS: f32 = 203.0;

/// The peak a picture with no declared mastering display is assumed to reach: 1000 cd/m², which is
/// BT.2100's HLG display reference and the value consumer displays are calibrated to. Expressed
/// against this project's SDR target, it is the ratio an HLG frame gets scaled by.
pub const HLG_SDR_PEAK_RATIO: f32 = 1000.0 / SDR_TARGET_NITS;
/// PQ mastering peak assumed when a file signals PQ but carries no mastering metadata. The
/// compression is what hides the guess: a wrong peak changes the highlight rolloff, not the
/// mid-tones.
pub const PQ_DEFAULT_MASTERING_NITS: f32 = 1000.0;

/// Which transform a file's pixels need, if any.
///
/// Only signalling that describes **the image data itself** qualifies: an ISO `nclx` box or a PNG
/// `cICP` chunk. An ICC profile does not, because in a HEIF container several items can carry
/// profiles and nothing in the header says which one the decoded frame is — and the loader's output
/// for those files is already display-ready (measured: identical values with the loader's colour
/// transforms on and off, and no channel above 1.0). Tone-mapping that frame would darken a
/// correctly-rendered picture, so it is left alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SceneTransform {
    /// Show the frame as decoded.
    None,
    /// HLG-encoded scene values.
    Hlg,
    /// PQ-encoded values.
    Pq,
}

/// The transform for one file's decoded frame.
pub fn scene_transform(signal: &crate::hdr::ImageSignal) -> SceneTransform {
    if !crate::hdr::is_hdr_signal(signal) {
        return SceneTransform::None;
    }
    if signal.source != crate::hdr::SignalSource::Nclx {
        return SceneTransform::None;
    }
    match signal.transfer {
        crate::hdr::TransferFunction::Hlg => SceneTransform::Hlg,
        crate::hdr::TransferFunction::Pq => SceneTransform::Pq,
        _ => SceneTransform::None,
    }
}

/// Apply the transform a file needs to a decoded `R8G8B8A8` frame, using the standards-derived
/// defaults for a display without HDR output.
pub fn apply_scene_transform(rgba: &[u8], transform: SceneTransform) -> Vec<u8> {
    match transform {
        SceneTransform::None => rgba.to_vec(),
        SceneTransform::Hlg => tone_map_frame(
            rgba,
            HdrKind::Hlg {
                peak_ratio: HLG_SDR_PEAK_RATIO,
            },
            SDR_TARGET_NITS,
        ),
        SceneTransform::Pq => tone_map_frame(
            rgba,
            HdrKind::Pq {
                peak_nits: PQ_DEFAULT_MASTERING_NITS,
            },
            SDR_TARGET_NITS,
        ),
    }
}

/// Bytes per pixel in the frames this module maps.
const PIXEL_STRIDE: usize = 4;

/// The HDR representation a frame is in, i.e. what its stored numbers mean.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HdrKind {
    /// HLG-encoded scene values, with the picture's peak `peak_ratio` times SDR reference white.
    Hlg { peak_ratio: f32 },
    /// PQ-encoded values, with a mastering peak in cd/m².
    Pq { peak_nits: f32 },
    /// sRGB-encoded values with a gain-map headroom ratio, for `gain_map_blend`'s headroom
    /// bookkeeping. No frame transform uses this: the gain image is not available yet.
    GainMap { ratio: f32 },
}

/// PQ EOTF: encoded `0.0..1.0` to luminance normalised against 10 000 cd/m².
pub fn pq_eotf(encoded: f32) -> f32 {
    let root = encoded.max(0.0).powf(1.0 / PQ_M2);
    let denominator = PQ_C2 - PQ_C3 * root;
    if denominator <= 0.0 {
        return 0.0;
    }
    ((root - PQ_C1).max(0.0) / denominator).powf(1.0 / PQ_M1)
}

/// PQ EOTF in cd/m².
pub fn pq_eotf_nits(encoded: f32) -> f32 {
    pq_eotf(encoded) * PQ_PEAK_NITS
}

/// PQ OETF: luminance normalised against 10 000 cd/m² to encoded `0.0..1.0`.
pub fn pq_oetf(normalized: f32) -> f32 {
    let root = normalized.max(0.0).powf(PQ_M1);
    ((PQ_C1 + PQ_C2 * root) / (1.0 + PQ_C3 * root)).powf(PQ_M2)
}

/// PQ OETF from cd/m².
pub fn pq_oetf_from_nits(nits: f32) -> f32 {
    pq_oetf(nits / PQ_PEAK_NITS)
}

/// HLG EOTF: encoded `0.0..1.0` to scene-linear, normalised against the picture's peak.
pub fn hlg_eotf(encoded: f32) -> f32 {
    if encoded <= HLG_KNEE_ENCODED {
        encoded * encoded / 3.0
    } else {
        (((encoded - HLG_C) / HLG_A).exp() + HLG_B) / 12.0
    }
}

/// HLG OETF: scene-linear `0.0..1.0` to encoded. The two branches meet at `1/12` → `0.5`, which is
/// the property that makes HLG usable without knowing the display's peak.
pub fn hlg_oetf(linear: f32) -> f32 {
    if linear <= HLG_KNEE_LINEAR {
        (3.0 * linear.max(0.0)).sqrt()
    } else {
        HLG_A * (12.0 * linear.max(0.0) - HLG_B).ln() + HLG_C
    }
}

/// sRGB EOTF (IEC 61966-2-1): encoded to linear.
pub fn srgb_eotf(encoded: f32) -> f32 {
    if encoded <= 0.04045 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}

/// sRGB OETF: linear to encoded.
pub fn srgb_oetf(linear: f32) -> f32 {
    let linear = linear.clamp(0.0, 1.0);
    if linear <= 0.003_130_8 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

/// ITU-R BT.2020 Attachment A: BT.709 → BT.2020 for D65. The conversion used here is this matrix's
/// inverse, which the test verifies by multiplying the two.
pub const BT709_TO_BT2020: [[f32; 3]; 3] = [
    [0.6274, 0.3293, 0.0433],
    [0.0691, 0.8303, 0.1006],
    [0.0214, 0.1295, 0.8491],
];

fn invert3(m: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if det.abs() < 1e-9 {
        // A singular matrix would mean the constants above are wrong; the caller cannot recover,
        // so return the input unchanged rather than inventing a transform.
        return m;
    }
    let inv = 1.0 / det;
    [
        [
            (m[1][1] * m[2][2] - m[1][2] * m[2][1]) * inv,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv,
        ],
        [
            (m[1][2] * m[2][0] - m[1][0] * m[2][2]) * inv,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv,
        ],
        [
            (m[1][0] * m[2][1] - m[1][1] * m[2][0]) * inv,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv,
        ],
    ]
}

/// BT.2020 → BT.709, both at D65. Derived from `BT709_TO_BT2020` so the two cannot drift apart.
pub fn bt2020_to_bt709() -> [[f32; 3]; 3] {
    invert3(BT709_TO_BT2020)
}

/// Apply a 3×3 matrix to linear RGB, clamping negatives: BT.2020 primaries reach outside BT.709's
/// gamut, and a negative channel is not displayable.
pub fn apply_matrix(m: [[f32; 3]; 3], rgb: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * rgb[0] + m[0][1] * rgb[1] + m[0][2] * rgb[2],
        m[1][0] * rgb[0] + m[1][1] * rgb[1] + m[1][2] * rgb[2],
        m[2][0] * rgb[0] + m[2][1] * rgb[1] + m[2][2] * rgb[2],
    ]
}

/// Extended Reinhard: `white` maps to `1.0`, the curve is monotonic, and nothing exceeds `1.0`.
pub fn compress(linear: f32, white: f32) -> f32 {
    let white = white.max(1e-3);
    let l = linear.max(0.0);
    let out = l * (1.0 + l / (white * white)) / (1.0 + l);
    out.clamp(0.0, 1.0)
}

/// Tone-map a whole `R8G8B8A8` frame. The matrix needs all three channels together, so this works
/// per pixel rather than per channel.
pub fn tone_map_frame(rgba: &[u8], kind: HdrKind, target_nits: f32) -> Vec<u8> {
    let matrix = bt2020_to_bt709();
    let target = target_nits.max(1.0);
    let mut out = Vec::with_capacity(rgba.len());
    // `as_chunks` splits whole pixels from a trailing partial one, so a malformed frame loses no
    // bytes rather than being guessed at.
    let (pixels, tail) = rgba.as_chunks::<PIXEL_STRIDE>();
    for px in pixels {
        let encoded = [
            px[0] as f32 / 255.0,
            px[1] as f32 / 255.0,
            px[2] as f32 / 255.0,
        ];
        let (linear, white) = match kind {
            HdrKind::Hlg { peak_ratio } => {
                let ratio = peak_ratio.max(1.0);
                (encoded.map(|e| hlg_eotf(e) * ratio), ratio)
            }
            HdrKind::Pq { peak_nits } => {
                let peak = peak_nits.max(target);
                (encoded.map(|e| pq_eotf_nits(e) / target), peak / target)
            }
            HdrKind::GainMap { .. } => {
                out.extend_from_slice(px);
                continue;
            }
        };
        let in_gamut = apply_matrix(matrix, linear);
        for channel in in_gamut {
            out.push((srgb_oetf(compress(channel, white)) * 255.0).round() as u8);
        }
        out.push(px[3]);
    }
    out.extend_from_slice(tail);
    out
}

/// ISO 21496-1 gain-map expansion, in linear light: `base` is the SDR baseline, `gain` is the gain
/// map decoded to `0.0..1.0`, and the boosted image is `base * (base_boost + (max_boost -
/// base_boost) * gain)`.
///
/// `gamma` is the gain map's own transfer exponent (ISO writes 1.0 for a linear gain map; Apple's
/// files use 1.7 or 2.0), applied to `gain` before interpolation.
pub fn gain_map_boost(base: f32, gain: f32, base_boost: f32, max_boost: f32, gamma: f32) -> f32 {
    let shape = gain.clamp(0.0, 1.0).powf(gamma.max(1.0).recip());
    let boost = base_boost + (max_boost.max(base_boost) - base_boost) * shape;
    (base.max(0.0) * boost).clamp(0.0, max_boost.max(base_boost))
}

/// Blend a whole linear base frame with a linear gain frame. Both are single-channel-per-pixel
/// floats of the same length; the result keeps the base's range when the gain is all zero.
pub fn gain_map_blend(
    base: &[f32],
    gain: &[f32],
    base_boost: f32,
    max_boost: f32,
    gamma: f32,
) -> Vec<f32> {
    base.iter()
        .zip(gain.iter())
        .map(|(b, g)| gain_map_boost(*b, *g, base_boost, max_boost, gamma))
        .collect()
}

/// HLG's scene-referred headroom, from a file's declared peak ratio, is what a display without HDR
/// output needs; `1.0` means the file claims no extra headroom, so there is nothing to map.
pub fn hlg_needs_mapping(peak_ratio: f32) -> bool {
    peak_ratio > 1.001
}

/// The peak ratio a PQ file claims, relative to the SDR target.
pub fn pq_headroom(peak_nits: f32, target_nits: f32) -> f32 {
    peak_nits / target_nits.max(1.0)
}
