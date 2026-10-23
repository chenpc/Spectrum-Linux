//! L5 cheap pixel assertions: structure, not a baseline diff.
//!
//! The snapshot tests catch a baseline drifting; these catch a screen that stopped *drawing*
//! something, and they say which colour went missing. Colours come from `theme::Tokens` (generated
//! from the design file), so the assertions follow the design instead of a hand-copied hex. The
//! tolerance is the snapshot's 2 LSB.
//!
//! Counts below are measured, not guessed: on a 1440×900 library screen with an empty folder's
//! placeholder tiles, the tile colour covers ~367 000 px; selecting a tile paints ~3 000 px of
//! accent (nothing paints accent before a selection); the detail canvas covers ~873 000 px of the
//! canvas colour while the library screen has ~145 px of it.

use std::cell::RefCell;
use std::rc::Rc;

use egui::Color32;
use egui_kittest::Harness;
use image::RgbaImage;
use spectrum_app::app::AppState;
use spectrum_app::theme::Tokens;
use spectrum_testkit::fixture;

fn fixture_dir(tag: &str) -> std::path::PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("spectrum-egui-pixels-{tag}"));
    std::fs::create_dir_all(&dir).unwrap_or_default();
    for i in 0..14 {
        fixture::write_png_fixture(&dir, &format!("p{i:02}.png"), 40, 30, [120 + i * 8, 60, 90]);
    }
    dir
}

fn harness(tag: &str) -> (Rc<RefCell<AppState>>, Harness<'static>) {
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir(tag))));
    let drawn = state.clone();
    let harness = Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .with_theme(egui::Theme::Light)
        .build_ui(move |ui| drawn.borrow_mut().render(ui));
    (state, harness)
}

fn frame(harness: &mut Harness<'static>) -> RgbaImage {
    harness
        .render()
        .unwrap_or_else(|err| panic!("the harness cannot render: {err}"))
}

/// Pixels within `tolerance` of a token colour, per channel — the snapshot test's tolerance.
fn count_of(image: &RgbaImage, colour: Color32, tolerance: i32) -> usize {
    image
        .pixels()
        .filter(|px| {
            let p = px.0;
            (p[0] as i32 - colour[0] as i32).abs() <= tolerance
                && (p[1] as i32 - colour[1] as i32).abs() <= tolerance
                && (p[2] as i32 - colour[2] as i32).abs() <= tolerance
        })
        .count()
}

fn distinct_colours(image: &RgbaImage) -> usize {
    let mut seen = std::collections::HashSet::new();
    for px in image.pixels() {
        seen.insert(px.0);
    }
    seen.len()
}

#[test]
fn neither_screen_is_blank() {
    let (state, mut harness) = harness("blank");
    harness.run();
    let grid = frame(&mut harness);
    assert_eq!(grid.width(), 1440);
    assert_eq!(grid.height(), 900);
    assert!(
        distinct_colours(&grid) > 200,
        "a screen with {} distinct colours is not drawing a UI",
        distinct_colours(&grid)
    );

    state
        .borrow_mut()
        .run("grid.click", serde_json::json!({ "index": 3 }));
    state.borrow_mut().run("detail.open", serde_json::json!({}));
    harness.run();
    let detail = frame(&mut harness);
    assert!(
        distinct_colours(&detail) > 200,
        "the detail view has only {} distinct colours",
        distinct_colours(&detail)
    );
}

#[test]
fn the_grid_area_is_filled_with_the_tile_colour() {
    let (_state, mut harness) = harness("tiles");
    harness.run();
    let grid = frame(&mut harness);
    let tokens = Tokens::photos();
    let filled = count_of(&grid, tokens.tile, 2);
    assert!(
        filled > 100_000,
        "the grid should fill a large part of the window with the tile placeholder colour \
         ({} px of {:?}); if this drops, the grid stopped drawing tiles",
        filled,
        tokens.tile
    );
    // A flat window would pass the colour check while still being broken.
    let white = count_of(&grid, tokens.window, 2);
    assert!(
        white > 100_000 && white < grid.width() as usize * grid.height() as usize * 7 / 10,
        "the window is neither mostly background nor uniformly filled: {white} px of window white"
    );
}

#[test]
fn selecting_paints_the_accent_and_nothing_does_before_it() {
    let (state, mut harness) = harness("accent");
    harness.run();
    let tokens = Tokens::photos();
    let before = count_of(&frame(&mut harness), tokens.accent, 2);
    assert!(
        before < 100,
        "the library screen paints accent ({before} px) before anything is selected, which the \
         design does not do — or the token changed meaning"
    );

    state
        .borrow_mut()
        .run("grid.click", serde_json::json!({ "index": 3 }));
    harness.run();
    let after = count_of(&frame(&mut harness), tokens.accent, 2);
    assert!(
        after > 1_000,
        "selecting a tile must paint the accent colour, got {after} px"
    );
}

#[test]
fn the_detail_canvas_uses_the_canvas_colour() {
    let (state, mut harness) = harness("canvas");
    harness.run();
    let tokens = Tokens::photos();
    let grid = frame(&mut harness);
    assert!(
        count_of(&grid, tokens.detail, 2) < 5_000,
        "the library screen should not be filled with the detail canvas colour"
    );

    state
        .borrow_mut()
        .run("grid.click", serde_json::json!({ "index": 0 }));
    harness.run();
    state.borrow_mut().run("detail.open", serde_json::json!({}));
    harness.run();
    let detail = frame(&mut harness);
    let canvas = count_of(&detail, tokens.detail, 2);
    assert!(
        canvas > 500_000,
        "the detail canvas should cover most of the window ({canvas} px of {:?})",
        tokens.detail
    );
}
