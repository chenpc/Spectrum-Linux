//! F06 in the UI: Enter opens the single-image view, the strip's buttons drive the registry, and
//! zoom and drag change where the picture is painted.
//!
//! Geometry tests attach the decode worker and wait for the frame, because a canvas with no image
//! has nothing to move.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use egui::{Modifiers, Rect, Vec2};
use egui_kittest::Harness;
use spectrum_app::app::AppState;
use spectrum_app::thumbs::Worker;
use spectrum_testkit::fixture;

const SCREEN_W: f32 = 1440.0;
/// Bigger than the detail canvas at 1440 px wide, so panning has room to move.
const PHOTO_W: u32 = 1600;

fn harness(state: Rc<RefCell<AppState>>) -> Harness<'static> {
    Harness::builder()
        .with_size(egui::vec2(SCREEN_W, 900.0))
        .with_theme(egui::Theme::Light)
        .build_ui(move |ui| state.borrow_mut().render(ui))
}

fn ids(state: &AppState) -> Vec<String> {
    state.widgets.iter().map(|w| w.id.clone()).collect()
}

fn rect_of(state: &AppState, id: &str) -> Option<Rect> {
    state.widgets.iter().find(|w| w.id == id).map(|w| w.rect)
}

fn press(harness: &mut Harness<'static>, key: egui::Key) {
    harness.input_mut().events.push(egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    harness.run();
}

fn press_with_modifiers(harness: &mut Harness<'static>, key: egui::Key, modifiers: Modifiers) {
    harness.input_mut().events.push(egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    });
    harness.run();
}

fn click_id(state: &Rc<RefCell<AppState>>, harness: &mut Harness<'static>, id: &str) {
    let center = rect_of(&state.borrow(), id)
        .unwrap_or(Rect::NOTHING)
        .center();
    harness.input_mut().events.extend([
        egui::Event::PointerMoved(center),
        egui::Event::PointerButton {
            pos: center,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        },
        egui::Event::PointerButton {
            pos: center,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        },
    ]);
    harness.run();
}

/// Press, move, release — in separate passes. egui decides `dragged()` from the press it recorded
/// in an earlier pass, so putting all the events in one batch produces a click instead.
fn drag(harness: &mut Harness<'static>, from: egui::Pos2, delta: Vec2) {
    let to = from + delta;
    // One pass of hover first: egui measures a drag from the press position it recorded in the
    // previous pass, so a click made just before this drag would otherwise be read as the drag's
    // own distance.
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(from));
    harness.run();
    harness.input_mut().events.extend([
        egui::Event::PointerMoved(from),
        egui::Event::PointerButton {
            pos: from,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        },
    ]);
    harness.run();
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(from + delta * 0.5));
    harness.run();
    harness
        .input_mut()
        .events
        .push(egui::Event::PointerMoved(to));
    harness.run();
    harness.input_mut().events.extend([
        egui::Event::PointerButton {
            pos: to,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        },
        egui::Event::PointerMoved(to),
    ]);
    harness.run();
}

fn photo_dir(tag: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("spectrum-egui-detail-{tag}"));
    std::fs::create_dir_all(&dir).unwrap_or_default();
    fixture::write_png_fixture(&dir, "photo.png", PHOTO_W, 1200, [30, 90, 200]);
    dir
}

fn mixed_dir(tag: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("spectrum-egui-detail-{tag}"));
    std::fs::create_dir_all(&dir).unwrap_or_default();
    fixture::write_png_fixture(&dir, "photo.png", 400, 300, [30, 90, 200]);
    fixture::write_mp4_fixture(&dir, "clip.mp4", 1, 1, 1, false);
    dir
}

fn open_one(state: &Rc<RefCell<AppState>>, harness: &mut Harness<'static>, index: usize) {
    state
        .borrow_mut()
        .run("grid.click", serde_json::json!({"index": index}));
    harness.run();
    state.borrow_mut().run("detail.open", serde_json::json!({}));
    harness.run();
}

/// Wait for the worker's full-size frame. `harness.run` does not advance wall-clock time on its
/// own, so the frame count and a short sleep together stand in for the repaint loop.
fn wait_for_frame(state: &Rc<RefCell<AppState>>, harness: &mut Harness<'static>) -> bool {
    for _ in 0..80 {
        harness.run();
        if state.borrow().has_detail_texture() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[test]
fn enter_opens_the_detail_view_and_escape_closes_it() {
    let state = Rc::new(RefCell::new(AppState::new(photo_dir("enter"))));
    let mut harness = harness(state.clone());
    harness.run();

    state
        .borrow_mut()
        .run("grid.click", serde_json::json!({"index": 0}));
    harness.run();
    assert!(
        ids(&state.borrow()).iter().any(|id| id == "panel:toolbar"),
        "the library screen draws its toolbar"
    );

    press(&mut harness, egui::Key::Enter);
    let open = ids(&state.borrow());
    assert!(
        open.iter().any(|id| id == "canvas:detail"),
        "Enter on one selected photo opens the canvas: {open:?}"
    );
    assert!(
        open.iter().any(|id| id == "button:detail-close"),
        "the strip offers a way back out"
    );
    assert!(
        !open.iter().any(|id| id == "panel:toolbar"),
        "the canvas replaces the toolbar rather than stacking under it"
    );
    assert!(
        !open.iter().any(|id| id.starts_with("tile:")),
        "and replaces the grid"
    );
    assert!(
        open.iter().any(|id| id == "panel:sidebar"),
        "the sidebar stays, so the app is still navigable"
    );

    press(&mut harness, egui::Key::Escape);
    let closed = ids(&state.borrow());
    assert!(
        !closed.iter().any(|id| id == "canvas:detail"),
        "Escape closes the canvas"
    );
    assert!(
        closed.iter().any(|id| id == "panel:toolbar"),
        "the library screen is back"
    );
}

#[test]
fn ctrl_plus_minus_zooms_from_the_keyboard() {
    let state = Rc::new(RefCell::new(AppState::new(photo_dir("keys"))));
    let mut harness = harness(state.clone());
    harness.run();
    open_one(&state, &mut harness, 0);

    let before = state.borrow().lib.detail_zoom;
    assert_eq!(before, None, "opening starts fitted");
    press_with_modifiers(
        &mut harness,
        egui::Key::Equals,
        Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        },
    );
    let zoomed = state.borrow().lib.detail_zoom;
    assert!(
        zoomed.is_some() && zoomed.unwrap_or(0.0) > 1.0,
        "Ctrl + `+` zooms in from the fit scale, got {zoomed:?}"
    );

    press_with_modifiers(
        &mut harness,
        egui::Key::Minus,
        Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        },
    );
    let back = state.borrow().lib.detail_zoom;
    assert!(
        back.is_some() && back.unwrap_or(99.0) < zoomed.unwrap_or(99.0),
        "Ctrl + `-` zooms back out, got {back:?} after {zoomed:?}"
    );

    // Escape closes the canvas before it clears the selection, so one keypress does two things in
    // the order a user expects.
    press(&mut harness, egui::Key::Escape);
    assert!(
        state.borrow().lib.detail.is_none(),
        "Escape closed the canvas"
    );
    assert!(
        !state.borrow().lib.selection.set.is_empty(),
        "and left the selection alone"
    );
    press(&mut harness, egui::Key::Escape);
    assert!(
        state.borrow().lib.selection.set.is_empty(),
        "a second Escape clears the selection"
    );
}

#[test]
fn the_close_button_returns_to_the_grid() {
    let state = Rc::new(RefCell::new(AppState::new(photo_dir("close"))));
    let mut harness = harness(state.clone());
    harness.run();
    open_one(&state, &mut harness, 0);
    assert!(
        state.borrow().lib.detail.is_some(),
        "the canvas is open before the click"
    );

    click_id(&state, &mut harness, "button:detail-close");
    assert_eq!(
        state.borrow().lib.detail,
        None,
        "the strip's Close ran detail.close"
    );
    assert_eq!(state.borrow().lib.detail_zoom, None);
    let drawn = ids(&state.borrow());
    assert!(
        drawn.iter().any(|id| id == "panel:toolbar"),
        "the grid screen is redrawn"
    );
}

#[test]
fn zoom_buttons_change_the_size_the_picture_is_painted_at() {
    let state = Rc::new(RefCell::new(AppState::new(photo_dir("zoom"))));
    state.borrow_mut().set_thumb_worker(Worker::spawn(220, 8));
    let mut harness = harness(state.clone());
    harness.run();
    open_one(&state, &mut harness, 0);
    assert!(
        wait_for_frame(&state, &mut harness),
        "the worker delivers a full-size frame"
    );

    let canvas = rect_of(&state.borrow(), "canvas:detail").expect("canvas:detail is registered");
    let fitted = state.borrow().canvas.image_rect;
    assert!(
        fitted.width() <= canvas.width() + 0.5 && fitted.height() <= canvas.height() + 0.5,
        "fit keeps the whole picture inside the canvas: {fitted:?} in {canvas:?}"
    );

    click_id(&state, &mut harness, "button:detail-zoom-in");
    let zoomed = state.borrow().canvas.image_rect;
    assert!(
        zoomed.width() > fitted.width() * 1.1,
        "the `+` button grows the picture from {} px to {} px",
        fitted.width(),
        zoomed.width()
    );
    assert!(
        zoomed.width() > canvas.width(),
        "zoomed past fit, the picture is larger than the canvas ({zoomed:?} in {canvas:?})"
    );

    click_id(&state, &mut harness, "button:detail-1x");
    let actual = state.borrow().canvas.image_rect;
    assert!(
        (actual.width() - PHOTO_W as f32).abs() < 1.0,
        "1:1 paints the picture at its own {PHOTO_W} px, got {:.1} px",
        actual.width()
    );

    click_id(&state, &mut harness, "button:detail-fit");
    assert_eq!(
        state.borrow().lib.detail_zoom,
        None,
        "Fit clears the explicit zoom"
    );
    let refit = state.borrow().canvas.image_rect;
    assert!(
        (refit.width() - fitted.width()).abs() < 1.0,
        "Fit comes back to the same size ({refit:?} vs {fitted:?})"
    );
}

#[test]
fn dragging_moves_the_picture_when_it_is_bigger_than_the_canvas() {
    let state = Rc::new(RefCell::new(AppState::new(photo_dir("drag"))));
    state.borrow_mut().set_thumb_worker(Worker::spawn(220, 8));
    let mut harness = harness(state.clone());
    harness.run();
    open_one(&state, &mut harness, 0);
    assert!(
        wait_for_frame(&state, &mut harness),
        "the worker delivers a full-size frame"
    );
    click_id(&state, &mut harness, "button:detail-1x");

    let canvas = rect_of(&state.borrow(), "canvas:detail").expect("canvas:detail is registered");
    let before = state.borrow().canvas.image_rect;
    assert!(
        before.width() > canvas.width() + 40.0,
        "at 1:1 the photo overflows the canvas, so there is something to pan: {before:?} in {canvas:?}"
    );

    drag(&mut harness, canvas.center(), egui::vec2(-70.0, 0.0));
    let after = state.borrow().canvas.image_rect;
    let moved = after.center().x - before.center().x;
    assert!(
        (moved + 70.0).abs() < 2.0,
        "dragging 70 px left moves the picture 70 px left, got {moved:.1} px"
    );
    assert!(
        (after.center().y - before.center().y).abs() < 0.6,
        "a horizontal drag does not move it vertically"
    );
    assert_eq!(
        state.borrow().lib.detail_zoom,
        Some(1.0),
        "panning is not a zoom command"
    );
}

#[test]
fn panning_stops_at_the_picture_edges() {
    let state = Rc::new(RefCell::new(AppState::new(photo_dir("clamp"))));
    state.borrow_mut().set_thumb_worker(Worker::spawn(220, 8));
    let mut harness = harness(state.clone());
    harness.run();
    open_one(&state, &mut harness, 0);
    assert!(
        wait_for_frame(&state, &mut harness),
        "the worker delivers a full-size frame"
    );
    click_id(&state, &mut harness, "button:detail-1x");

    let canvas = rect_of(&state.borrow(), "canvas:detail").expect("canvas:detail is registered");
    let slack = (state.borrow().canvas.image_rect.width() - canvas.width()) / 2.0;
    assert!(slack > 100.0, "there is {slack:.0} px of picture to pan");

    // Farther than the picture can go: the right edge must not leave the canvas' left side, which
    // would show a strip of background and read as a broken image.
    drag(
        &mut harness,
        canvas.center(),
        egui::vec2(-(slack + 300.0), 0.0),
    );
    let after = state.borrow().canvas.image_rect;
    let moved = after.center().x - canvas.center().x;
    assert!(
        moved < 0.0,
        "the drag moved the picture left, {moved:.1} px"
    );
    assert!(
        (moved + slack).abs() < 1.0,
        "the pan stopped at the edge, not past it: moved {moved:.1} px of {slack:.1} px available"
    );
    assert!(
        after.min.x <= canvas.min.x + 0.5 && after.max.x >= canvas.max.x - 0.5,
        "the canvas stays covered by the picture, never by background: {after:?} in {canvas:?}"
    );
    assert!(
        (after.center().y - canvas.center().y).abs() < 1.0,
        "a horizontal drag leaves the vertical centring alone"
    );
}

#[test]
fn a_video_opens_a_canvas_with_no_picture_but_stays_addressable() {
    let state = Rc::new(RefCell::new(AppState::new(mixed_dir("video"))));
    let mut harness = harness(state.clone());
    harness.run();

    // The tile's own click, through the registry — the same route a script uses.
    let video = rect_of(&state.borrow(), "tile:clip.mp4").expect("the video has a tile");
    let center = video.center();
    harness.input_mut().events.extend([
        egui::Event::PointerMoved(center),
        egui::Event::PointerButton {
            pos: center,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        },
        egui::Event::PointerButton {
            pos: center,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        },
    ]);
    harness.run();
    assert!(
        state.borrow().lib.sole().is_some(),
        "clicking the video tile selects it alone"
    );

    press(&mut harness, egui::Key::Enter);
    let drawn = ids(&state.borrow());
    assert!(
        drawn.iter().any(|id| id == "canvas:detail"),
        "a video still opens the detail view: {drawn:?}"
    );
    assert!(
        drawn.iter().any(|id| id == "button:detail-zoom-in"),
        "and the strip is addressable, so a script can drive it"
    );
    assert_eq!(
        state.borrow().canvas.image_rect,
        Rect::NOTHING,
        "no picture is painted — video decoding is F16, so the canvas shows its placeholder"
    );
    assert!(
        !state.borrow().has_detail_texture(),
        "and nothing was uploaded for it"
    );
}
