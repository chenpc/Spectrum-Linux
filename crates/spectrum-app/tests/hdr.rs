//! F07 in the UI: HDR presentation changes the pixels a tile draws, and only for files whose
//! signalling describes the image data itself.
//!
//! The fixture is a PNG with a `cICP` chunk, which is how a file can declare HLG while its stored
//! pixels are the HLG-encoded signal — and this project's loader hands those bytes back unchanged
//! (measured), so the tile's colour is predictable.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use egui::Rect;
use egui_kittest::Harness;
use image::RgbaImage;
use spectrum_app::app::AppState;
use spectrum_app::thumbs::Worker;
use spectrum_testkit::fixture;

const BT2020: u8 = 9;
const HLG: u8 = 18;
const SRGB: u8 = 13;
/// The encoded orange both fixtures store.
const ORANGE: [u8; 3] = [188, 120, 60];
/// The same pixel after HLG tone-mapping at BT.2100's 1000 cd/m² reference peak onto a 203-nit SDR
/// display — worked out independently of this code.
const ORANGE_TONE_MAPPED: [u8; 3] = [217, 130, 36];

fn fixture_dir(tag: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("spectrum-egui-hdr-{tag}"));
    std::fs::create_dir_all(&dir).unwrap_or_default();
    fixture::write_png_fixture_cicp(&dir, "hlg_orange.png", 40, 30, ORANGE, BT2020, HLG);
    fixture::write_png_fixture_cicp(&dir, "sdr_orange.png", 40, 30, ORANGE, BT2020, SRGB);
    dir
}

fn harness(state: Rc<RefCell<AppState>>) -> Harness<'static> {
    Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .with_theme(egui::Theme::Light)
        .build_ui(move |ui| state.borrow_mut().render(ui))
}

fn rect_of(state: &AppState, id: &str) -> Option<Rect> {
    state.widgets.iter().find(|w| w.id == id).map(|w| w.rect)
}

/// `Harness::render` hands back a `Result`; a failure here means the headless renderer is broken,
/// which is a test-infrastructure failure rather than something the assertions can express.
fn rendered_frame(harness: &mut Harness<'static>) -> RgbaImage {
    harness
        .render()
        .unwrap_or_else(|err| panic!("the harness cannot render: {err}"))
}

fn render_scale(image: &RgbaImage, width_points: f32) -> f32 {
    image.width() as f32 / width_points
}

/// Sample points inside a tile, in render coordinates. Deliberately off-centre: the harness marks
/// the fake pointer position at the centre.
fn probes(state: &AppState, id: &str, scale: f32) -> Vec<(usize, usize)> {
    let Some(center) = rect_of(state, id).map(|r| r.center()) else {
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

fn dominant(image: &RgbaImage, points: &[(usize, usize)]) -> [u8; 4] {
    let mut counts: std::collections::HashMap<[u8; 4], usize> = std::collections::HashMap::new();
    for (x, y) in points {
        *counts
            .entry(image.get_pixel(*x as u32, *y as u32).0)
            .or_default() += 1;
    }
    counts
        .iter()
        .max_by_key(|(_, count)| *count)
        .map(|(pixel, _)| *pixel)
        .unwrap_or([0, 0, 0, 0])
}

/// Draw until the tile holds decoded pixels in either presentation, and return the last frame.
fn wait_decoded(
    state: &Rc<RefCell<AppState>>,
    harness: &mut Harness<'static>,
    id: &str,
) -> Option<RgbaImage> {
    for _ in 0..80 {
        harness.run();
        let rendered = rendered_frame(harness);
        let scale = render_scale(&rendered, harness.ctx.viewport_rect().width());
        let points = probes(&state.borrow(), id, scale);
        let colour = dominant(&rendered, &points);
        if !points.is_empty()
            && (matches(&colour, ORANGE, 2) || matches(&colour, ORANGE_TONE_MAPPED, 2))
        {
            return Some(rendered);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    None
}

#[test]
fn hdr_presentation_replaces_the_tile_pixels() {
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir("toggle"))));
    state.borrow_mut().set_thumb_worker(Worker::spawn(164, 8));
    let mut harness = harness(state.clone());
    harness.run();

    // HDR presentation is on by default, so the HLG tile arrives tone-mapped and the sRGB tile does
    // not change at all.
    let image = wait_decoded(&state, &mut harness, "tile:hlg_orange.png")
        .expect("the HLG fixture decodes into its tile");
    let scale = render_scale(&image, harness.ctx.viewport_rect().width());
    let points = probes(&state.borrow(), "tile:hlg_orange.png", scale);
    assert!(
        matches(&dominant(&image, &points), ORANGE_TONE_MAPPED, 3,),
        "with HDR presentation on, the HLG tile shows the tone-mapped colour: {:?}",
        dominant(&image, &points)
    );
    let sdr_points = probes(&state.borrow(), "tile:sdr_orange.png", scale);
    assert!(
        matches(&dominant(&image, &sdr_points), ORANGE, 3),
        "an sRGB-declared PNG keeps its own pixels: {:?}",
        dominant(&image, &sdr_points)
    );

    assert!(state.borrow().lib.hdr_enabled, "HDR presentation starts on");
    state.borrow_mut().run("hdr.toggle", serde_json::json!({}));
    assert!(!state.borrow().lib.hdr_enabled);
    harness.run();
    harness.run();
    let off = rendered_frame(&mut harness);
    let scale_off = render_scale(&off, harness.ctx.viewport_rect().width());
    let points_off = probes(&state.borrow(), "tile:hlg_orange.png", scale_off);
    assert!(
        matches(&dominant(&off, &points_off), ORANGE, 3),
        "with HDR presentation off the tile shows the file's own encoded colour: {:?}",
        dominant(&off, &points_off)
    );
    // The sRGB tile is unaffected by the toggle in either direction.
    let sdr_off = probes(&state.borrow(), "tile:sdr_orange.png", scale_off);
    assert!(
        matches(&dominant(&off, &sdr_off), ORANGE, 3),
        "the sRGB tile did not change when the mode changed: {:?}",
        dominant(&off, &sdr_off)
    );

    state.borrow_mut().run("hdr.toggle", serde_json::json!({}));
    harness.run();
    let back = rendered_frame(&mut harness);
    let scale_back = render_scale(&back, harness.ctx.viewport_rect().width());
    let points_back = probes(&state.borrow(), "tile:hlg_orange.png", scale_back);
    assert!(
        matches(&dominant(&back, &points_back), ORANGE_TONE_MAPPED, 3),
        "turning it back on maps the picture again: {:?}",
        dominant(&back, &points_back)
    );
}

#[test]
fn the_detail_view_tone_maps_the_open_file_too() {
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir("detail"))));
    state.borrow_mut().set_thumb_worker(Worker::spawn(220, 8));
    let mut harness = harness(state.clone());
    harness.run();

    state
        .borrow_mut()
        .run("grid.click", serde_json::json!({"index": 0}));
    harness.run();
    state.borrow_mut().run("detail.open", serde_json::json!({}));
    for _ in 0..80 {
        harness.run();
        if state.borrow().has_detail_texture() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        state.borrow().has_detail_texture(),
        "the detail view got a full-size frame"
    );
    let canvas = rect_of(&state.borrow(), "canvas:detail").expect("canvas:detail is registered");
    let rendered = rendered_frame(&mut harness);
    let scale = render_scale(&rendered, harness.ctx.viewport_rect().width());
    let centre = egui::pos2(canvas.center().x, canvas.center().y - 90.0);
    let probe = (
        (centre.x * scale).round() as usize,
        (centre.y * scale).round() as usize,
    );
    let pixel = rendered.get_pixel(probe.0 as u32, probe.1 as u32).0;
    assert!(
        matches(&pixel, ORANGE_TONE_MAPPED, 4),
        "the canvas shows the tone-mapped picture: {pixel:?}"
    );

    state.borrow_mut().run("hdr.toggle", serde_json::json!({}));
    for _ in 0..10 {
        harness.run();
    }
    let off = rendered_frame(&mut harness);
    let scale_off = render_scale(&off, harness.ctx.viewport_rect().width());
    let probe_off = (
        (centre.x * scale_off).round() as usize,
        (centre.y * scale_off).round() as usize,
    );
    let pixel_off = off.get_pixel(probe_off.0 as u32, probe_off.1 as u32).0;
    assert!(
        matches(&pixel_off, ORANGE, 4),
        "with presentation off the canvas shows the file's own pixels: {pixel_off:?}"
    );
}
