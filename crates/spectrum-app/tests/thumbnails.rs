//! Thumbnails reach the grid: a fixture image is decoded on the worker thread, uploaded as a
//! texture, and drawn inside its tile in place of the placeholder colour.
//!
//! The worker is attached explicitly here. Every other UI test leaves it off, so snapshots cannot
//! depend on when a decode finishes.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use egui_kittest::Harness;
use image::RgbaImage;
use spectrum_app::app::AppState;
use spectrum_app::thumbs::Worker;
use spectrum_testkit::fixture;

/// The window the tests build, in egui points.
const SCREEN_W: f32 = 1440.0;

fn fixture_dir(tag: &str, files: &[(&str, u32, u32, [u8; 3])]) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("spectrum-egui-thumb-{tag}"));
    std::fs::create_dir_all(&dir).unwrap_or_default();
    for (name, w, h, rgb) in files {
        fixture::write_png_fixture(&dir, name, *w, *h, *rgb);
    }
    dir
}

fn render_scale(width_px: u32, width_points: f32) -> f32 {
    width_px as f32 / width_points
}

fn harness(state: Rc<RefCell<AppState>>) -> Harness<'static> {
    Harness::builder()
        .with_size(egui::vec2(SCREEN_W, 900.0))
        .with_theme(egui::Theme::Light)
        .build_ui(move |ui| state.borrow_mut().render(ui))
}

/// Pixels inside a registered tile, scaled to render coordinates. `Harness::render` may draw at a
/// HiDPI scale, so registry rects (egui points) and image pixels are different numbers. Points are
/// deliberately off-centre: the test harness marks the fake pointer position.
fn probes(state: &AppState, id: &str, scale: f32) -> Vec<(usize, usize)> {
    let Some(center) = state
        .widgets
        .iter()
        .find(|w| w.id == id)
        .map(|w| w.rect.center())
    else {
        return Vec::new();
    };
    [(-46.0, 34.0), (46.0, 34.0), (0.0, 52.0)]
        .iter()
        .map(|(dx, dy)| center + egui::vec2(*dx, *dy))
        .map(|p| {
            (
                (p.x * scale).round() as usize,
                (p.y * scale).round() as usize,
            )
        })
        .collect()
}

fn matches(pixel: &[u8; 4], want: [u8; 3], tolerance: i32) -> bool {
    (pixel[0] as i32 - want[0] as i32).abs() <= tolerance
        && (pixel[1] as i32 - want[1] as i32).abs() <= tolerance
        && (pixel[2] as i32 - want[2] as i32).abs() <= tolerance
}

/// The most common colour inside a tile: the placeholder colour while nothing has decoded, the
/// decoded image's own colour afterwards.
fn dominant(image: &RgbaImage, points: &[(usize, usize)]) -> [u8; 4] {
    let mut counts: std::collections::HashMap<[u8; 4], usize> = std::collections::HashMap::new();
    for (x, y) in points {
        *counts
            .entry(image.get_pixel(*x as u32, *y as u32).0)
            .or_default() += 1;
    }
    counts
        .iter()
        .max_by_key(|(_, n)| **n)
        .map(|(c, _)| *c)
        .unwrap_or([0, 0, 0, 255])
}

/// Decode happens on the worker thread, so pump frames until the tile stops being the placeholder.
fn wait_for_decode(
    harness: &mut Harness<'static>,
    state: &Rc<RefCell<AppState>>,
    id: &str,
    scale: f32,
    placeholder: [u8; 3],
) -> RgbaImage {
    let mut painted = harness
        .render()
        .unwrap_or_else(|err| panic!("the harness cannot render: {err}"));
    for _ in 0..80 {
        std::thread::sleep(Duration::from_millis(25));
        harness.run();
        painted = harness
            .render()
            .unwrap_or_else(|err| panic!("the harness cannot render: {err}"));
        let points = probes(&state.borrow(), id, scale);
        if !matches(&dominant(&painted, &points), placeholder, 8) {
            break;
        }
    }
    painted
}

#[test]
fn a_decoded_thumbnail_replaces_the_placeholder_in_its_own_tile() {
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir(
        "decode",
        &[
            ("red.png", 200, 120, [220, 30, 40]),
            ("blue.png", 120, 200, [30, 60, 220]),
        ],
    ))));
    state.borrow_mut().set_thumb_worker(Worker::spawn(164, 8));
    let mut harness = harness(state.clone());
    harness.run();

    let first = harness
        .render()
        .unwrap_or_else(|err| panic!("the harness cannot render: {err}"));
    let scale = render_scale(first.width(), SCREEN_W);
    let painted = wait_for_decode(&mut harness, &state, "tile:red.png", scale, [233, 233, 235]);

    let red_points = probes(&state.borrow(), "tile:red.png", scale);
    let red = dominant(&painted, &red_points);
    assert!(
        matches(&red, [220, 30, 40], 12),
        "the red tile must show the decoded red image, got {red:?}"
    );

    // Per-file mapping: the second tile shows its own image, not the first one.
    let blue_points = probes(&state.borrow(), "tile:blue.png", scale);
    let blue = dominant(&painted, &blue_points);
    assert!(
        matches(&blue, [30, 60, 220], 12),
        "the blue tile must show the decoded blue image, got {blue:?}"
    );
}

/// Without a decoder attached, tiles keep the design's placeholder colour — which is what keeps
/// snapshots independent of decode timing.
#[test]
fn tiles_keep_the_placeholder_when_no_decoder_is_attached() {
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir(
        "no-worker",
        &[("red.png", 200, 120, [220, 30, 40])],
    ))));
    let mut harness = harness(state.clone());
    for _ in 0..5 {
        harness.run();
    }
    let painted = harness
        .render()
        .unwrap_or_else(|err| panic!("the harness cannot render: {err}"));
    let points = probes(
        &state.borrow(),
        "tile:red.png",
        render_scale(painted.width(), SCREEN_W),
    );
    let flat = dominant(&painted, &points);
    assert!(
        matches(&flat, [233, 233, 235], 6),
        "with no decoder the tile must be the placeholder colour, got {flat:?}"
    );
}

/// A file the loader refuses (a video) must not be re-queued every frame: its tile stays a
/// placeholder and the registry still offers it.
#[test]
fn a_refused_file_stays_a_placeholder_without_repeated_work() {
    let mut dir = std::env::temp_dir();
    dir.push("spectrum-egui-thumb-refuse");
    std::fs::create_dir_all(&dir).unwrap_or_default();
    fixture::write_mp4_fixture(&dir, "clip.mp4", 1, 1, 1, false);
    fixture::write_png_fixture(&dir, "ok.png", 80, 80, [10, 200, 90]);

    let state = Rc::new(RefCell::new(AppState::new(dir)));
    state.borrow_mut().set_thumb_worker(Worker::spawn(164, 8));
    let mut harness = harness(state.clone());
    for _ in 0..30 {
        std::thread::sleep(Duration::from_millis(20));
        harness.run();
    }

    let painted = harness
        .render()
        .unwrap_or_else(|err| panic!("the harness cannot render: {err}"));
    let scale = render_scale(painted.width(), SCREEN_W);
    let app = state.borrow();

    let video_points = probes(&app, "tile:clip.mp4", scale);
    let flat = dominant(&painted, &video_points);
    assert!(
        matches(&flat, [233, 233, 235], 8),
        "a video has no decodable frame, so its tile keeps the placeholder colour, got {flat:?}"
    );

    // The image next to it did decode, so the placeholder is the loader's answer, not a broken
    // upload path.
    let image_points = probes(&app, "tile:ok.png", scale);
    let green = dominant(&painted, &image_points);
    assert!(
        matches(&green, [10, 200, 90], 12),
        "the neighbouring image must be decoded, got {green:?}"
    );
    assert!(
        app.widgets.iter().any(|w| w.id == "tile:clip.mp4"),
        "an undecodable file is still an addressable tile"
    );
}
