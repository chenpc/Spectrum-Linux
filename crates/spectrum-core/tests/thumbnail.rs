//! Thumbnail sizing and cache behaviour (`ThumbnailService`).

use spectrum_core::thumbnail;
use spectrum_testkit::fixture;

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("spectrum-core-{tag}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn scale_within_caps_and_preserves_aspect() {
    assert_eq!(thumbnail::scale_within(100, 100, 48), (48, 48));
    assert_eq!(thumbnail::scale_within(200, 100, 64), (64, 32));
    assert_eq!(thumbnail::scale_within(100, 50, 64), (64, 32));
    // never upscale
    assert_eq!(thumbnail::scale_within(8, 6, 64), (8, 6));
    // long side is the one that gets capped
    assert_eq!(thumbnail::scale_within(300, 100, 60), (60, 20));
}

#[test]
fn second_fetch_hits_cache() {
    let dir = temp_dir("thumb_cache");
    let path = fixture::write_png_fixture(&dir, "thumb.png", 100, 60, [255, 0, 0]);

    let mut service = thumbnail::ThumbnailService::new(64);
    let (pixels, first) = service.get(&path).expect("decode");
    assert!(!first.cached);
    assert_eq!(
        pixels.rgba.len(),
        (first.width as usize * first.height as usize) * 4,
        "the cache holds tightly packed RGBA at the target size"
    );
    assert!(
        pixels.rgba[0] > 200 && pixels.rgba[1] < 60 && pixels.rgba[2] < 60,
        "a 100×60 red PNG must come back red: {:?}",
        &pixels.rgba[..4]
    );
    assert_eq!((first.source_width, first.source_height), (100, 60));
    assert_eq!((first.width, first.height), (64, 38));

    let (_, second) = service.get(&path).expect("cache hit");
    assert!(second.cached);
    assert_eq!((second.width, second.height), (64, 38));
    assert_eq!(service.hits(), 1);
    assert_eq!(service.decodes(), 1, "second request must not decode again");
    assert_eq!(service.len(), 1);
}

#[test]
fn small_images_are_not_upscaled_and_cache_is_per_file() {
    let dir = temp_dir("thumb_small");
    let small = fixture::write_png_fixture(&dir, "small.png", 8, 6, [0, 255, 0]);
    let big = fixture::write_png_fixture(&dir, "big.png", 120, 40, [0, 0, 255]);

    let mut service = thumbnail::ThumbnailService::new(64);
    let (_, t) = service.get(&small).unwrap();
    assert_eq!((t.width, t.height), (8, 6));
    let (_, t) = service.get(&big).unwrap();
    assert_eq!((t.width, t.height), (64, 21));

    // re-fetch both: two hits, still two decodes
    service.get(&small).unwrap();
    service.get(&big).unwrap();
    assert_eq!(service.hits(), 2);
    assert_eq!(service.decodes(), 2);
    assert!(service.is_cached(&small) && service.is_cached(&big));
}

/// A known 4×2 source averages into 2×1 in a way that catches a wrong stride, a wrong channel
/// count, or a filter that samples one pixel instead of a block.
#[test]
fn downscale_averages_each_block() {
    // Two rows of four RGBA pixels: left half opaque black, right half opaque white.
    let mut src: Vec<u8> = Vec::new();
    for _ in 0..2 {
        src.extend_from_slice(&[
            0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255,
        ]);
    }
    let out = thumbnail::downscale(&src, 4, 2, 16, 4, 2, 1);
    assert_eq!(out.len(), 8);
    assert_eq!(&out[..4], &[0, 0, 0, 255], "left block is black");
    assert_eq!(&out[4..], &[255, 255, 255, 255], "right block is white");

    // The same two pixels as R8G8B8 with a stride wider than the row: padding must be ignored.
    let mut rgb: Vec<u8> = vec![0, 0, 0, 255, 255, 255];
    rgb.extend_from_slice(&[9, 9, 9, 9, 9, 9]);
    let out = thumbnail::downscale(&rgb, 2, 1, 12, 3, 2, 1);
    assert_eq!(&out[..4], &[0, 0, 0, 255]);
    assert_eq!(&out[4..], &[255, 255, 255, 255]);

    // A 4×4 gradient halved to 2×2: each output is the average of its 2×2 block.
    let mut grad: Vec<u8> = Vec::new();
    for y in 0..4u8 {
        for x in 0..4u8 {
            grad.extend_from_slice(&[x * 80, y * 80, 0, 255]);
        }
    }
    let out = thumbnail::downscale(&grad, 4, 4, 16, 4, 2, 2);
    assert_eq!(
        &out[0..4],
        &[40, 40, 0, 255],
        "top-left block averages (0,0),(80,0),(0,80),(80,80)"
    );
    assert_eq!(
        &out[12..16],
        &[200, 200, 0, 255],
        "bottom-right block averages (240,240)…"
    );

    // Nothing to do, and an unusable channel count, both give an empty buffer rather than a panic.
    assert!(thumbnail::downscale(&src, 0, 2, 16, 4, 2, 1).is_empty());
    assert!(thumbnail::downscale(&src, 4, 2, 16, 2, 2, 1).is_empty());
}

/// A wide source loses its sides, a tall one loses its top and bottom, and a square one is
/// untouched — which is what keeps a 4:3 photo from being squashed into a square cell.
#[test]
fn crop_center_square_keeps_the_middle() {
    let mut rgba: Vec<u8> = Vec::new();
    for y in 0..2u8 {
        for x in 0..4u8 {
            rgba.extend_from_slice(&[x * 60, y * 200, 0, 255]);
        }
    }
    let wide = spectrum_core::thumbnail::crop_center_square(&spectrum_core::thumbnail::Decoded {
        width: 4,
        height: 2,
        rgba: rgba.clone(),
    });
    assert_eq!((wide.width, wide.height), (2, 2));
    assert_eq!(
        &wide.rgba[..4],
        &[60, 0, 0, 255],
        "columns 1 and 2 of the source"
    );
    assert_eq!(&wide.rgba[4..8], &[120, 0, 0, 255]);
    assert_eq!(wide.rgba.len(), 16);

    // A square stays as it is (no copy, no crop).
    let square = spectrum_core::thumbnail::crop_center_square(&spectrum_core::thumbnail::Decoded {
        width: 2,
        height: 2,
        rgba: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
    });
    assert_eq!(square.rgba.len(), 16);
    assert_eq!(&square.rgba[..4], &[1, 2, 3, 4]);

    // A buffer shorter than its own dimensions is returned as-is rather than read out of bounds.
    let short = spectrum_core::thumbnail::crop_center_square(&spectrum_core::thumbnail::Decoded {
        width: 4,
        height: 2,
        rgba: vec![0, 0, 0, 255],
    });
    assert_eq!((short.width, short.height), (4, 2));
}
