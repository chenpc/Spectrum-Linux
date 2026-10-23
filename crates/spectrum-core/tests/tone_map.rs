//! Tone-mapping against published reference values, and against hand-computed pixel results.
//!
//! The tolerance is one 8-bit step (`1/255`), which is what `docs/testing.md` L7 asks for: a
//! reference value must survive the whole path from an encoded channel to a byte, not just the
//! floating-point math.

use spectrum_core::thumbnail::ThumbnailService;
use spectrum_core::tone_map::{self as tm, HdrKind};
use spectrum_testkit::fixture;

const TOLERANCE: i32 = 1;

fn close_tight(got: f32, want: f32, tolerance: f32, what: &str) {
    assert!(
        (got - want).abs() <= tolerance,
        "{what}: got {got}, want {want} ± {tolerance}"
    );
}

fn pixel(bytes: &[u8]) -> [u8; 3] {
    [bytes[0], bytes[1], bytes[2]]
}

fn assert_pixel(got: [u8; 3], want: [u8; 3], what: &str) {
    for (channel, (g, w)) in got.iter().zip(want.iter()).enumerate() {
        let diff = (*g as i32 - *w as i32).abs();
        assert!(
            diff <= TOLERANCE,
            "{what} channel {channel}: got {got:?}, want {want:?} (off by {diff})"
        );
    }
}

#[test]
fn pq_matches_the_published_reference_values() {
    // ST 2084 / BT.2100: 100 cd/m² encodes to 0.5080762.
    close_tight(
        tm::pq_oetf_from_nits(100.0),
        0.508_076_2,
        2e-6,
        "100 nits encodes",
    );
    // The same value read back is 100 nits, so the pair is not a coincidence of rounding.
    close_tight(
        tm::pq_eotf_nits(0.508_076_2),
        100.0,
        0.05,
        "0.5080762 decodes",
    );
    // 0.75 is the "1000 cd/m²" checkpoint used across the HDR tooling.
    close_tight(
        tm::pq_eotf_nits(0.75),
        1000.0,
        30.0,
        "0.75 decodes to about 1000 nits",
    );
    close_tight(tm::pq_eotf_nits(0.0), 0.0, 1e-4, "black");
    close_tight(
        tm::pq_eotf_nits(1.0),
        10_000.0,
        1.0,
        "code 1.0 is PQ's peak",
    );

    let mut previous = -1.0;
    for step in 0..=255 {
        let value = tm::pq_eotf_nits(step as f32 / 255.0);
        assert!(
            value >= previous,
            "PQ EOTF must be monotonic at step {step}"
        );
        previous = value;
    }

    for step in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
        let back = tm::pq_oetf(tm::pq_eotf(step));
        close_tight(back, step, 2e-5, "PQ round-trip");
    }
}

#[test]
fn hlg_is_continuous_at_its_knee_and_scene_referred() {
    close_tight(
        tm::hlg_eotf(0.5),
        1.0 / 12.0,
        1e-7,
        "HLG 0.5 decodes to 1/12 of peak",
    );
    close_tight(
        tm::hlg_oetf(tm::HLG_KNEE_LINEAR),
        0.5,
        1e-7,
        "the knee encodes to 0.5",
    );
    // Both branches agree just either side of the knee — a discontinuity there would show as a
    // band in a gradient.
    let below = tm::hlg_oetf(tm::HLG_KNEE_LINEAR - 1e-6);
    let above = tm::hlg_oetf(tm::HLG_KNEE_LINEAR + 1e-6);
    close_tight(below, above, 1e-5, "HLG OETF is continuous across the knee");

    for step in [0.0f32, 0.25, 0.5, 0.75, 1.0] {
        close_tight(
            tm::hlg_oetf(tm::hlg_eotf(step)),
            step,
            1e-6,
            "HLG round-trip",
        );
        close_tight(
            tm::hlg_eotf(tm::hlg_oetf(step)),
            step,
            1e-6,
            "HLG round-trip, other way",
        );
    }
    let mut previous = -1.0;
    for step in 0..=255 {
        let value = tm::hlg_eotf(step as f32 / 255.0);
        assert!(
            value >= previous,
            "HLG EOTF must be monotonic at step {step}"
        );
        previous = value;
    }
    // HLG is scene-referred: mid grey is a small fraction of peak, which is why a peak ratio is
    // needed before it can be shown on an SDR display.
    assert!(tm::hlg_eotf(0.5) < 0.1, "mid grey is not near peak");
    assert!(
        !tm::hlg_needs_mapping(1.0),
        "a file with no headroom needs no map"
    );
    assert!(tm::hlg_needs_mapping(6.0));
}

#[test]
fn srgb_round_trips_and_is_continuous() {
    for step in [0.0f32, 0.003_130_8, 0.040_45, 0.5, 1.0] {
        close_tight(
            tm::srgb_oetf(tm::srgb_eotf(step)),
            step,
            1e-6,
            "sRGB round-trip",
        );
    }
    close_tight(
        tm::srgb_eotf(0.5),
        0.214_041_1,
        1e-5,
        "sRGB 0.5 is 21.4% linear",
    );
}

#[test]
// Matrix multiplication reads best with indices, which is exactly what this lint dislikes.
#[allow(clippy::needless_range_loop)]
fn the_primary_matrices_are_mutual_inverses() {
    let forward = tm::BT709_TO_BT2020;
    let back = tm::bt2020_to_bt709();
    for row in 0..3 {
        for col in 0..3 {
            let product: f32 = (0..3).map(|k| back[row][k] * forward[k][col]).sum();
            let want = if row == col { 1.0 } else { 0.0 };
            close_tight(product, want, 1e-3, "BT.2020→BT.709 times BT.709→BT.2020");
        }
    }
    // D65 white is the same in both, so a neutral grey must stay neutral. This is what stops the
    // converted image from going green or magenta.
    let neutral = tm::apply_matrix(back, [1.0, 1.0, 1.0]);
    for channel in neutral {
        close_tight(channel, 1.0, 1e-3, "neutral stays neutral");
    }
    // ITU-R BT.2020 Attachment 2 prints BT.2020→BT.709's first row as 1.66680, −0.13614, −0.02124.
    let red = tm::apply_matrix(back, [1.0, 0.0, 0.0]);
    close_tight(red[0], 1.666_80, 1e-4, "BT.2020 red to BT.709 r");
    close_tight(red[1], -0.136_14, 1e-4, "BT.2020 red to BT.709 g");
    close_tight(red[2], -0.021_24, 1e-4, "BT.2020 red to BT.709 b");
}

#[test]
fn compression_is_monotonic_ends_at_one_and_starts_at_zero() {
    assert_eq!(tm::compress(0.0, 4.9), 0.0, "black stays black");
    close_tight(tm::compress(4.9, 4.9), 1.0, 1e-3, "the peak maps to 1.0");
    let mut previous = -1.0;
    for step in 0..200 {
        let value = tm::compress(step as f32 / 20.0, 4.9);
        assert!(value >= previous, "compression must be monotonic at {step}");
        assert!(value <= 1.0, "compression must never exceed 1.0");
        previous = value;
    }
    // Everything above the peak lands on 1.0 rather than wrapping.
    assert_eq!(tm::compress(100.0, 4.9), 1.0);
    assert_eq!(
        tm::compress(-1.0, 4.9),
        0.0,
        "an out-of-gamut negative channel becomes black"
    );
}

#[test]
fn gain_map_boost_interpolates_between_base_and_max() {
    // ISO 21496-1's shape: gain 0 is the baseline, gain 1 is the full headroom.
    close_tight(
        tm::gain_map_boost(0.5, 0.0, 1.0, 4.0, 1.0),
        0.5,
        1e-6,
        "zero gain",
    );
    close_tight(
        tm::gain_map_boost(0.5, 1.0, 1.0, 4.0, 1.0),
        2.0,
        1e-6,
        "full gain",
    );
    // Boost is applied to the base pixel: gain 0.25 interpolates the boost to 1.75, so 0.5 → 0.875.
    close_tight(
        tm::gain_map_boost(0.5, 0.25, 1.0, 4.0, 1.0),
        0.875,
        1e-6,
        "quarter gain",
    );
    // A gamma on the gain map means the boost curve is not linear in the stored gain.
    let shaped = tm::gain_map_boost(0.5, 0.25, 1.0, 4.0, 2.0);
    assert!(
        (shaped - 1.25).abs() < 1e-6,
        "gamma 2 decodes the same stored gain to 0.5, so the boost is 2.5 and the pixel 1.25, got {shaped}"
    );
    close_tight(
        tm::gain_map_boost(0.0, 1.0, 1.0, 4.0, 1.0),
        0.0,
        1e-6,
        "black gains nothing",
    );
    let mut previous = -1.0;
    for step in 0..=100 {
        let value = tm::gain_map_boost(0.4, step as f32 / 100.0, 1.0, 8.0, 1.7);
        assert!(
            value >= previous,
            "boost must grow with gain at step {step}"
        );
        assert!(
            value <= 8.0 * 0.4 + 1e-5,
            "boost must stay inside its headroom"
        );
        previous = value;
    }

    let base = vec![0.0, 0.25, 0.5, 1.0];
    let no_gain = vec![0.0; 4];
    let blended = tm::gain_map_blend(&base, &no_gain, 1.0, 4.0, 1.0);
    assert_eq!(
        blended, base,
        "an all-zero gain map leaves the baseline alone"
    );

    let full = vec![1.0; 4];
    let boosted = tm::gain_map_blend(&base, &full, 1.0, 4.0, 1.0);
    let expected: Vec<f32> = base.iter().map(|b| b * 4.0).collect();
    assert_eq!(boosted, expected, "a full gain map gives the full boost");

    // Mismatched lengths are not an error to guess about: zip stops at the shorter one.
    assert_eq!(tm::gain_map_blend(&base, &[1.0], 1.0, 4.0, 1.0).len(), 1);
}

#[test]
fn a_frame_is_mapped_to_the_hand_computed_sdr_values() {
    // Every number below was worked out by hand from the standard's coefficients, then confirmed
    // against the module: HLG encoded 0.5 → 1/12 → ×6 = 0.5 linear → neutral after the matrix →
    // extended Reinhard with white 6 → 0.33796 → sRGB encode → 0.6154 → 157.
    let encoded_half = vec![128u8, 128, 128, 255];
    let mapped = tm::tone_map_frame(&encoded_half, HdrKind::Hlg { peak_ratio: 6.0 }, 203.0);
    assert_eq!(
        mapped.len(),
        encoded_half.len(),
        "a frame keeps its byte count"
    );
    // 0.5 in 8-bit is 128/255 = 0.50196, so the reference point is 157 (±1) on all three channels.
    assert_pixel(pixel(&mapped), [157, 157, 157], "HLG mid grey at ratio 6");

    let warm = vec![188, 120, 60, 255];
    let mapped = tm::tone_map_frame(&warm, HdrKind::Hlg { peak_ratio: 6.0 }, 203.0);
    assert_pixel(
        pixel(&mapped),
        [222, 140, 40],
        "an HLG-encoded orange at ratio 6",
    );

    let black = vec![0, 0, 0, 255];
    let mapped = tm::tone_map_frame(&black, HdrKind::Hlg { peak_ratio: 6.0 }, 203.0);
    assert_pixel(pixel(&mapped), [0, 0, 0], "black");

    let white = vec![255, 255, 255, 255];
    let mapped = tm::tone_map_frame(&white, HdrKind::Hlg { peak_ratio: 6.0 }, 203.0);
    assert_pixel(
        pixel(&mapped),
        [255, 255, 255],
        "the picture's peak maps to full white",
    );
    assert_eq!(mapped[3], 255, "alpha survives untouched");

    // PQ: 100 nits (the SDR reference white) is the same 157 after tone-mapping onto 203 nits,
    // because both are "reference white, one display apart".
    let encoded_100 = (tm::pq_oetf_from_nits(tm::SDR_REFERENCE_NITS) * 255.0).round() as u8;
    let frame = vec![encoded_100, encoded_100, encoded_100, 255];
    let mapped = tm::tone_map_frame(
        &frame,
        HdrKind::Pq { peak_nits: 1000.0 },
        tm::SDR_TARGET_NITS,
    );
    assert_pixel(
        pixel(&mapped),
        [157, 157, 157],
        &format!("a PQ 100-nit pixel (encoded {encoded_100}) on a 203-nit SDR display"),
    );

    // The SDR display's own peak (203 nits) is bright but not clipped.
    let encoded_203 = (tm::pq_oetf_from_nits(tm::SDR_TARGET_NITS) * 255.0).round() as u8;
    let frame = vec![encoded_203, encoded_203, encoded_203, 255];
    let mapped = tm::tone_map_frame(
        &frame,
        HdrKind::Pq { peak_nits: 1000.0 },
        tm::SDR_TARGET_NITS,
    );
    assert_pixel(pixel(&mapped), [191, 191, 191], "a PQ 203-nit pixel");
    assert!(
        pixel(&mapped)[0] < 255,
        "the SDR peak must not already be clipped"
    );

    let mapped = tm::tone_map_frame(
        &white,
        HdrKind::Pq { peak_nits: 1000.0 },
        tm::SDR_TARGET_NITS,
    );
    assert_pixel(pixel(&mapped), [255, 255, 255], "PQ's mastering peak");
}

#[test]
fn a_gain_map_frame_passes_through_untouched_until_the_gain_image_exists() {
    // The base image of a gain-map file is already a display-ready SDR signal. Lifting it without
    // the gain image would wash the whole picture, so this is deliberately a no-op — documented in
    // `tone_map.rs` and in `docs/backlog.md` F07.
    let frame = vec![188, 120, 60, 200, 10, 20, 30, 255];
    let mapped = tm::tone_map_frame(&frame, HdrKind::GainMap { ratio: 4.0 }, 203.0);
    assert_eq!(mapped, frame, "gain-map signalling alone changes no pixels");
}

#[test]
fn tone_mapping_keeps_a_ramp_in_order() {
    // A ramp must stay ordered after tone-mapping: a bright pixel coming out darker than a dim one
    // is the classic tone-map bug.
    let mut frame = Vec::new();
    for step in 0..=255u8 {
        frame.extend_from_slice(&[step, step, step, 255]);
    }
    for kind in [
        HdrKind::Hlg { peak_ratio: 6.0 },
        HdrKind::Hlg { peak_ratio: 1.2 },
        HdrKind::Pq { peak_nits: 1000.0 },
        HdrKind::Pq { peak_nits: 4000.0 },
    ] {
        let mapped = tm::tone_map_frame(&frame, kind, tm::SDR_TARGET_NITS);
        let mut previous = -1i32;
        for (index, px) in mapped.as_chunks::<4>().0.iter().enumerate() {
            let value = px[0] as i32;
            assert!(
                value >= previous,
                "{kind:?}: step {index} came out darker than step {}",
                index - 1
            );
            previous = value;
        }
        assert_eq!(
            mapped[0], 0,
            "{kind:?}: the first step of the ramp is black"
        );
        assert_eq!(
            *mapped.last().unwrap_or(&0),
            255u8,
            "alpha of the last pixel"
        );
        assert_eq!(
            mapped[(255 * 4) as usize],
            255,
            "{kind:?}: the top of the ramp is white"
        );
    }
}

#[test]
fn png_cicp_is_read_and_is_an_hdr_signal() {
    let dir = std::env::temp_dir().join("spectrum-tone-cicp");
    std::fs::create_dir_all(&dir).unwrap();
    let hlg = fixture::write_png_fixture_cicp(&dir, "hlg.png", 24, 18, [188, 120, 60], 9, 18);
    let pq = fixture::write_png_fixture_cicp(&dir, "pq.png", 24, 18, [188, 120, 60], 9, 16);
    let sdr = fixture::write_png_fixture_cicp(&dir, "sdr.png", 24, 18, [188, 120, 60], 1, 13);
    let plain = fixture::write_png_fixture(&dir, "plain.png", 24, 18, [188, 120, 60]);

    let data = std::fs::read(&hlg).unwrap();
    let cicp = spectrum_core::png::cicp(&data).expect("the fixture has a cICP chunk");
    assert_eq!(cicp.primaries, 9, "BT.2020");
    assert_eq!(cicp.transfer, 18, "HLG");
    assert_eq!(cicp.matrix, 0);
    assert!(cicp.full_range);

    let signal = spectrum_core::scan::detect_signal(&hlg).expect("an HLG PNG has a signal");
    assert_eq!(signal.describe(), "HLG (nclx)");
    assert!(spectrum_core::hdr::is_hdr_signal(&signal));
    let signal = spectrum_core::scan::detect_signal(&pq).expect("a PQ PNG has a signal");
    assert!(spectrum_core::hdr::is_hdr_signal(&signal));
    let signal = spectrum_core::scan::detect_signal(&sdr).expect("an sRGB PNG has a signal");
    assert!(
        !spectrum_core::hdr::is_hdr_signal(&signal),
        "sRGB primaries and transfer are not HDR"
    );
    assert_eq!(
        spectrum_core::scan::detect_signal(&plain),
        None,
        "a PNG with no cICP chunk claims nothing"
    );
}

#[test]
fn png_chunks_that_are_not_four_bytes_are_not_read() {
    let dir = std::env::temp_dir().join("spectrum-tone-cicp-bad");
    std::fs::create_dir_all(&dir).unwrap();
    let path = fixture::write_png_fixture_cicp(&dir, "ok.png", 8, 8, [1, 2, 3], 9, 18);
    let mut data = std::fs::read(&path).unwrap();
    // Shorten the cICP chunk's length field: the chunk is then malformed and must be ignored
    // rather than half-read.
    let at = data
        .windows(4)
        .position(|w| w == b"cICP")
        .expect("the fixture has a cICP chunk");
    data[at - 4] = 2;
    assert_eq!(
        spectrum_core::png::cicp(&data),
        None,
        "a 2-byte cICP is malformed"
    );

    let mut other = data.clone();
    let at = other.windows(4).position(|w| w == b"cICP").unwrap();
    other[at - 4] = 5;
    assert_eq!(
        spectrum_core::png::cicp(&other),
        None,
        "a 5-byte cICP is malformed"
    );

    assert_eq!(
        spectrum_core::png::chunks(b"not a png file at all").len(),
        0
    );
    assert_eq!(spectrum_core::png::cicp(b"not a png file at all"), None);
}

#[test]
fn a_decoded_hdr_png_tone_maps_to_the_same_numbers() {
    // End to end: the fixture's stored bytes are the HLG-encoded signal (the loader passes them
    // through unchanged — measured), so the tone-mapped tile must equal the hand-computed result.
    let dir = std::env::temp_dir().join("spectrum-tone-decode");
    std::fs::create_dir_all(&dir).unwrap();
    let path = fixture::write_png_fixture_cicp(&dir, "orange.png", 40, 30, [188, 120, 60], 9, 18);

    let mut service = ThumbnailService::new(64);
    let (decoded, _) = service.get(&path).expect("glycin decodes the fixture");
    assert_eq!(
        pixel(&decoded.rgba),
        [188, 120, 60],
        "the loader hands back the file's own encoded values"
    );

    let mapped = tm::tone_map_frame(
        &decoded.rgba,
        HdrKind::Hlg { peak_ratio: 6.0 },
        tm::SDR_TARGET_NITS,
    );
    assert_pixel(
        pixel(&mapped),
        [222, 140, 40],
        "the decoded frame tone-mapped for display",
    );
    assert_eq!(
        mapped.len(),
        decoded.rgba.len(),
        "tone-mapping changes values, not layout"
    );
    // The SDR path stays exactly the file's own pixels.
    let plain = tm::tone_map_frame(&decoded.rgba, HdrKind::GainMap { ratio: 6.0 }, 203.0);
    assert_eq!(plain, decoded.rgba, "no transform means no change");
}

#[test]
fn headroom_helpers_describe_the_display_gap() {
    close_tight(
        tm::pq_headroom(1000.0, 203.0),
        1000.0 / 203.0,
        1e-3,
        "PQ headroom",
    );
    assert!(
        tm::pq_headroom(203.0, 203.0) <= 1.001,
        "at the display's peak there is nothing to squeeze"
    );
    assert_eq!(
        tm::pq_headroom(1000.0, 0.0),
        1000.0,
        "a zero target is clamped to 1"
    );
    assert!(!tm::hlg_needs_mapping(1.0));
    assert!(tm::hlg_needs_mapping(1.01));
}

#[test]
fn only_scene_referred_signalling_gets_a_transform() {
    let dir = std::env::temp_dir().join("spectrum-tone-scene");
    std::fs::create_dir_all(&dir).unwrap();
    let hlg = fixture::write_png_fixture_cicp(&dir, "hlg.png", 12, 12, [188, 120, 60], 9, 18);
    let pq = fixture::write_png_fixture_cicp(&dir, "pq.png", 12, 12, [188, 120, 60], 9, 16);
    let sdr = fixture::write_png_fixture_cicp(&dir, "sdr.png", 12, 12, [188, 120, 60], 1, 13);

    let transform =
        tm::scene_transform(&spectrum_core::scan::detect_signal(&hlg).expect("HLG PNG signal"));
    assert_eq!(transform, tm::SceneTransform::Hlg);
    let transform =
        tm::scene_transform(&spectrum_core::scan::detect_signal(&pq).expect("PQ signal"));
    assert_eq!(transform, tm::SceneTransform::Pq);
    let transform =
        tm::scene_transform(&spectrum_core::scan::detect_signal(&sdr).expect("sRGB signal"));
    assert_eq!(
        transform,
        tm::SceneTransform::None,
        "sRGB needs no transform"
    );

    // An HDR signal that came from an ICC profile describes a profile, not necessarily the decoded
    // frame, so the frame is shown as the loader produced it.
    // `nclx` says "unspecified" here, so the HDR signal can only come from the ICC's `cicp` tag.
    let heif = fixture::write_heif_fixture_with_icc(
        &dir,
        "icc_hdr.heic",
        2,
        2,
        0,
        true,
        None,
        Some((12, 16, 0, 1)),
        None,
    );
    let signal =
        spectrum_core::scan::detect_signal(&heif).expect("the fixture signals through its ICC");
    assert_eq!(
        signal.source,
        spectrum_core::hdr::SignalSource::IccCicp,
        "the fixture's HDR signal is ICC-derived, not nclx"
    );
    assert_eq!(
        tm::scene_transform(&signal),
        tm::SceneTransform::None,
        "ICC-derived HDR signalling does not authorize a tone-map"
    );
}

#[test]
fn the_default_transform_uses_the_standards_derived_headroom() {
    let dir = std::env::temp_dir().join("spectrum-tone-default");
    std::fs::create_dir_all(&dir).unwrap();
    let path = fixture::write_png_fixture_cicp(&dir, "orange.png", 40, 30, [188, 120, 60], 9, 18);
    let mut service = ThumbnailService::new(64);
    let (decoded, _) = service.get(&path).expect("decodes");

    // BT.2100 puts HLG's reference peak at 1000 cd/m²; against a 203-nit SDR target that is a ratio
    // of 4.926, which turns this encoded orange into 217/130/36. Worked out independently.
    assert!(
        (tm::HLG_SDR_PEAK_RATIO - 4.926_108).abs() < 1e-4,
        "the HLG ratio is 1000/203, got {}",
        tm::HLG_SDR_PEAK_RATIO
    );
    let mapped = tm::apply_scene_transform(&decoded.rgba, tm::SceneTransform::Hlg);
    assert_pixel(
        pixel(&mapped),
        [217, 130, 36],
        "HLG at the standards-derived peak",
    );
    assert_eq!(
        tm::apply_scene_transform(&decoded.rgba, tm::SceneTransform::None),
        decoded.rgba,
        "SceneTransform::None is a copy, not a transform"
    );

    let grey = vec![128u8, 128, 128, 255];
    let mapped = tm::apply_scene_transform(&grey, tm::SceneTransform::Hlg);
    assert_pixel(
        pixel(&mapped),
        [148, 148, 148],
        "HLG mid grey at the default peak",
    );
}
