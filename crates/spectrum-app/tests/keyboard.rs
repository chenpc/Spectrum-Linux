//! Keyboard navigation in the real screen: arrow/Home/End/Escape go through the same command
//! registry as clicks do, a focused text field keeps the arrow keys, and the keyboard-selected tile
//! is brought into view.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use egui::{Modifiers, Rect};
use egui_kittest::Harness;
use spectrum_app::app::AppState;
use spectrum_testkit::fixture;

fn fixture_dir(tag: &str, count: usize) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("spectrum-egui-keys-{tag}"));
    std::fs::create_dir_all(&dir).unwrap_or_default();
    for i in 0..count {
        fixture::write_png_fixture(&dir, &format!("p{i:02}.png"), 60, 60, [40, 90, 160]);
    }
    dir
}

fn harness(state: Rc<RefCell<AppState>>) -> Harness<'static> {
    Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .with_theme(egui::Theme::Light)
        .build_ui(move |ui| state.borrow_mut().render(ui))
}

/// Put a key into this pass's raw input — what `raw_input_hook` does in the window — and draw.
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

/// Type text into whatever holds focus.
fn type_text(harness: &mut Harness<'static>, text: &str) {
    harness
        .input_mut()
        .events
        .push(egui::Event::Text(text.to_string()));
    harness.run();
}

/// The same pointer sequence the control channel builds for `ui.click`, from the registry rect.
fn click_id(state: &Rc<RefCell<AppState>>, harness: &mut Harness<'static>, id: &str) {
    let center = rect_of(state, id).unwrap_or(Rect::NOTHING).center();
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

fn sole_name(state: &Rc<RefCell<AppState>>) -> Option<String> {
    let app = state.borrow();
    app.lib.sole().map(|index| app.lib.entries[index].name())
}

fn position_of_visible(state: &Rc<RefCell<AppState>>) -> Option<usize> {
    let app = state.borrow();
    let sole = app.lib.sole()?;
    app.lib.visible().iter().position(|&pos| pos == sole)
}

/// How many tiles are in each drawn row.
#[allow(dead_code)]
fn tile_rows(state: &Rc<RefCell<AppState>>) -> Vec<usize> {
    let app = state.borrow();
    let mut rows: Vec<f32> = Vec::new();
    let mut counts: Vec<usize> = Vec::new();
    for w in app.widgets.iter().filter(|w| w.kind == "tile") {
        match counts
            .iter_mut()
            .zip(rows.iter_mut())
            .find(|(_, y)| (**y - w.rect.min.y).abs() < 1.0)
        {
            Some((count, _)) => *count += 1,
            None => {
                rows.push(w.rect.min.y);
                counts.push(1);
            }
        }
    }
    counts
}

fn rect_of(state: &Rc<RefCell<AppState>>, id: &str) -> Option<Rect> {
    state
        .borrow()
        .widgets
        .iter()
        .find(|w| w.id == id)
        .map(|w| w.rect)
}

/// The column count the drawn grid is actually using, counted from the tile rects: the kittest
/// harness insets the window by its outer margin, so it is one column fewer than a real 1440 px
/// window.
fn drawn_columns(state: &Rc<RefCell<AppState>>) -> usize {
    let app = state.borrow();
    let tiles: Vec<Rect> = app
        .widgets
        .iter()
        .filter(|w| w.kind == "tile")
        .map(|w| w.rect)
        .collect();
    let mut rows: Vec<Vec<Rect>> = Vec::new();
    for rect in tiles {
        match rows
            .iter_mut()
            .find(|row| (row[0].min.y - rect.min.y).abs() < 1.0)
        {
            Some(row) => row.push(rect),
            None => rows.push(vec![rect]),
        }
    }
    rows.iter().map(|row| row.len()).max().unwrap_or(1)
}

#[test]
fn arrows_step_by_one_item_and_by_one_row() {
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir("step", 12))));
    let mut harness = harness(state.clone());
    harness.run();

    assert_eq!(
        state.borrow().lib.visible().len(),
        12,
        "every fixture is visible"
    );
    assert!(
        rect_of(&state, "panel:inspector").is_none(),
        "with nothing selected there is no inspector column, so the grid is wider than it is later"
    );

    // Nothing selected: the first arrow lands on the first item in display order, whatever its
    // name is (the default sort is by modification time, not by name).
    press(&mut harness, egui::Key::ArrowRight);
    assert_eq!(position_of_visible(&state), Some(0));
    let first = sole_name(&state).unwrap_or_default();
    // Now the inspector is up, and the column count the grid uses with it is the one the keys move
    // by — measured from the drawn rows rather than assumed.
    let columns = drawn_columns(&state);
    assert!(
        columns >= 2,
        "the grid should draw several columns, got {columns}"
    );
    press(&mut harness, egui::Key::ArrowRight);
    assert_eq!(position_of_visible(&state), Some(1));
    assert_ne!(sole_name(&state).as_deref(), Some(first.as_str()));

    press(&mut harness, egui::Key::ArrowDown);
    assert_eq!(
        position_of_visible(&state),
        Some(1 + columns),
        "Down moves one row, not one item"
    );
    press(&mut harness, egui::Key::ArrowUp);
    assert_eq!(position_of_visible(&state), Some(1));
    press(&mut harness, egui::Key::ArrowLeft);
    assert_eq!(position_of_visible(&state), Some(0));
}

#[test]
fn home_and_end_jump_and_escape_clears_the_selection() {
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir("ends", 8))));
    let mut harness = harness(state.clone());
    harness.run();

    press(&mut harness, egui::Key::End);
    assert_eq!(position_of_visible(&state), Some(7));
    press(&mut harness, egui::Key::Home);
    assert_eq!(position_of_visible(&state), Some(0));

    press(&mut harness, egui::Key::Escape);
    assert_eq!(state.borrow().lib.selection.count(), 0);
    assert!(
        rect_of(&state, "panel:inspector").is_none(),
        "no selection means no inspector column"
    );
}

#[test]
fn an_edge_move_changes_nothing() {
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir("edge", 6))));
    let mut harness = harness(state.clone());
    harness.run();

    press(&mut harness, egui::Key::Home);
    press(&mut harness, egui::Key::ArrowLeft);
    assert_eq!(position_of_visible(&state), Some(0), "already first");
    press(&mut harness, egui::Key::End);
    press(&mut harness, egui::Key::ArrowRight);
    assert_eq!(position_of_visible(&state), Some(5), "already last");
}

#[test]
fn the_keyboard_selected_tile_is_brought_into_the_scroll_view() {
    // Enough files that the grid really scrolls, so "into view" is a claim and not a tautology.
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir("scroll", 42))));
    let mut harness = harness(state.clone());
    harness.run();

    let scroll_before = rect_of(&state, "grid:scroll").expect("grid:scroll is registered");
    let first = rect_of(&state, "tile:p00.png").expect("the first tile is registered");
    assert!(
        first.max.y > scroll_before.max.y,
        "the last rows must start outside the view for this test to mean anything: \
         tile {first:?}, scroll {scroll_before:?}"
    );

    press(&mut harness, egui::Key::End);
    // The jump is applied to the next pass; give it a frame or two to settle.
    harness.run_steps(4);
    let name = sole_name(&state).expect("End selects a tile");
    let tile = rect_of(&state, &format!("tile:{name}")).expect("the selected tile is registered");
    let scroll = rect_of(&state, "grid:scroll").expect("grid:scroll is registered");
    assert!(
        scroll.contains(tile.center()),
        "the keyboard-selected tile must be inside the scroll view: tile {tile:?}, scroll {scroll:?}"
    );
}

#[test]
fn a_focused_path_field_keeps_the_arrow_keys_and_a_click_never_adds_a_folder() {
    let target = fixture_dir("field-target", 1);
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir("field", 4))));
    let mut harness = harness(state.clone());
    harness.run();

    let folders_before = state.borrow().lib.folders.len();
    click_id(&state, &mut harness, "field:add-path");
    type_text(&mut harness, &target.to_string_lossy());

    press(&mut harness, egui::Key::ArrowRight);
    press(&mut harness, egui::Key::ArrowDown);
    assert_eq!(
        state.borrow().lib.selection.count(),
        0,
        "the arrow keys belong to the focused text field, not the grid"
    );

    // A click in the field only focuses it. It used to submit the typed path, so clicking in order
    // to type silently changed the library.
    click_id(&state, &mut harness, "field:add-path");
    assert_eq!(
        state.borrow().lib.folders.len(),
        folders_before,
        "clicking the field does not add {}",
        target.display()
    );
    assert!(
        state
            .borrow()
            .lib
            .folders
            .iter()
            .all(|slot| slot.path != target),
        "the typed path must not have been added"
    );
}

#[test]
fn enter_in_the_focused_field_submits_the_path() {
    let target = fixture_dir("enter-target", 1);
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir("enter", 2))));
    let mut harness = harness(state.clone());
    harness.run();

    click_id(&state, &mut harness, "field:add-path");
    type_text(&mut harness, &target.to_string_lossy());
    eprintln!(
        "before Enter: add_path={:?} notice={:?}",
        state.borrow().add_path(),
        state.borrow().notice
    );
    press(&mut harness, egui::Key::Enter);
    eprintln!(
        "after Enter: folders={} add_path={:?} notice={:?}",
        state.borrow().lib.folders.len(),
        state.borrow().add_path(),
        state.borrow().notice
    );

    assert_eq!(
        state.borrow().lib.folders.len(),
        2,
        "Enter submits the typed path (notice: {:?})",
        state.borrow().notice
    );
    assert_eq!(state.borrow().lib.folder(), target.as_path());
    // The field is emptied after a successful add, so Enter does not add it twice.
    press(&mut harness, egui::Key::Enter);
    assert_eq!(state.borrow().lib.folders.len(), 2);
}
