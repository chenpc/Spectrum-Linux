//! L4: addressability. Every interactive widget must report a stable id and rect each frame, and
//! the ids the design document promises must be there. A missing id is a failure, not a skip.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use egui_kittest::Harness;
use spectrum_app::app::AppState;
use spectrum_testkit::fixture;

const BT2020: u16 = 9;
const SRGB: u16 = 13;
const BT709: u16 = 1;
const HLG: u16 = 18;

fn fixture_dir(tag: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("spectrum-egui-reg-{tag}"));
    std::fs::create_dir_all(&dir).unwrap_or_default();
    let hlg = fixture::write_fixture(&dir, "hlg_photo.heic", BT2020, HLG, 0, true);
    let sdr = fixture::write_fixture(&dir, "sdr_photo.heic", BT709, SRGB, 0, false);
    let _ = hlg;
    let _ = sdr;
    dir
}

fn harness(state: Rc<RefCell<AppState>>) -> Harness<'static> {
    Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .with_theme(egui::Theme::Light)
        .build_ui(move |ui| state.borrow_mut().render(ui))
}

fn ids(state: &AppState) -> Vec<String> {
    state.widgets.iter().map(|w| w.id.clone()).collect()
}

/// Read a fenced block (`​```ids` or `​```ids-on-selection`) out of `docs/ui-spec.md`. The spec is
/// the checklist the screen is graded against, the way FigCraft grades command ids with
/// `cargo xtask parity`. A missing file or an empty block is a failure, not an empty pass: a
/// vacuous checklist is worse than no checklist.
fn promised_ids(fence: &str) -> Vec<String> {
    let path = std::env::var("SPECTRUM_UI_SPEC")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/ui-spec.md").into());
    let spec = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("cannot read the UI spec at {path}: {err}"));
    let mut inside = false;
    let mut out = Vec::new();
    for line in spec.lines() {
        if line.trim() == format!("```{fence}") {
            inside = true;
            continue;
        }
        if inside {
            if line.trim() == "```" {
                break;
            }
            let id = line.trim();
            if !id.is_empty() && !id.starts_with('#') {
                out.push(id.to_string());
            }
        }
    }
    assert!(
        !out.is_empty(),
        "no `{fence}` block found in {path}: the checklist would pass vacuously"
    );
    out
}

#[test]
fn every_frame_registers_the_promised_ids() {
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir("all"))));
    let mut harness = harness(state.clone());
    harness.run();

    let drawn = ids(&state.borrow());
    let registered = drawn.len();
    assert!(
        drawn.len() > 30,
        "a full screen registers 30+ widgets, got {}",
        drawn.len()
    );
    for id in promised_ids("ids") {
        assert!(
            drawn.iter().any(|have| have.starts_with(id.as_str())),
            "promised id {id} is not on screen ({registered} widgets registered)"
        );
    }
}

#[test]
fn every_registered_rect_sits_inside_the_window() {
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir("bounds"))));
    let mut harness = harness(state.clone());
    harness.run();

    let screen = harness.ctx.viewport_rect();
    for widget in &state.borrow().widgets {
        assert!(
            screen.contains(widget.rect.center()),
            "{} registered at {:?}, outside the {screen:?} window",
            widget.id,
            widget.rect
        );
    }
}

#[test]
fn a_widget_that_leaves_the_screen_leaves_the_registry() {
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir("switch"))));
    let mut harness = harness(state.clone());
    harness.run();

    let before = ids(&state.borrow());
    assert!(
        before.iter().any(|id| id.starts_with("tile:")),
        "the All view draws tiles: {before:?}"
    );
    assert!(
        !before.iter().any(|id| id.starts_with("year:")),
        "the All view has no year rows"
    );

    state
        .borrow_mut()
        .run("view.set", serde_json::json!({"view": "years"}));
    harness.run();
    let after = ids(&state.borrow());
    assert!(
        after.iter().any(|id| id.starts_with("year:")),
        "Years view draws year rows: {after:?}"
    );
    assert!(
        !after.iter().any(|id| id.starts_with("tile:")),
        "Years view with nothing open draws no tiles — stale ids would make a dead widget clickable"
    );
    assert!(
        after.iter().any(|id| id == "panel:sidebar"),
        "panels persist across views"
    );
}

#[test]
fn the_inspector_only_offers_what_a_selection_makes_possible() {
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir("select"))));
    let mut harness = harness(state.clone());
    harness.run();

    // The library screen in the design has no inspector column at all: it appears only once
    // something is selected, and its ids come from the spec's `ids-on-selection` block.
    let none = ids(&state.borrow());
    for id in promised_ids("ids-on-selection") {
        assert!(
            !none.iter().any(|have| have.starts_with(id.as_str())),
            "{id} belongs to the inspector, which a library screen with no selection does not draw"
        );
    }

    state
        .borrow_mut()
        .run("grid.click", serde_json::json!({"index": 0}));
    harness.run();
    let one = ids(&state.borrow());
    assert!(
        one.iter().any(|id| id == "badge:inspector"),
        "a sole selection draws the badge and the edit controls"
    );
    let registered = one.len();
    for id in promised_ids("ids-on-selection") {
        assert!(
            one.iter().any(|have| have.starts_with(id.as_str())),
            "selected, so {id} must be on screen ({registered} widgets registered)"
        );
    }
}

/// A folder of `n` photos in one month, so the grid has to wrap.
fn fixture_dir_many(tag: &str, n: usize) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("spectrum-egui-reg-{tag}"));
    std::fs::create_dir_all(&dir).unwrap_or_default();
    for i in 0..n {
        fixture::write_fixture(&dir, &format!("p{i:02}.heic"), BT709, SRGB, 0, false);
    }
    dir
}

/// The design's grid is a 164 px cell with a 2 px gap, and it fills the panel width. Two bugs this
/// catches: a `Grid` whose trailing columns collapsed to the width of the HDR badge (tiles started
/// ~122 px inside their neighbours), and a wrapped row that ran unbounded inside a scroll area.
#[test]
fn tiles_are_on_a_166_px_pitch_and_wrap_inside_the_grid() {
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir_many("flow", 12))));
    let mut harness = harness(state.clone());
    harness.run();

    let app = state.borrow();
    let scroll = app
        .widgets
        .iter()
        .find(|w| w.id == "grid:scroll")
        .expect("grid:scroll is registered");
    let mut tiles: Vec<&spectrum_app::widgets::Widget> = app
        .widgets
        .iter()
        .filter(|w| w.id.starts_with("tile:"))
        .collect();
    assert_eq!(tiles.len(), 12, "every photo gets a tile");
    tiles.sort_by(|a, b| {
        a.rect
            .min
            .y
            .total_cmp(&b.rect.min.y)
            .then(a.rect.min.x.total_cmp(&b.rect.min.x))
    });

    let pitch = 166.0; // 164 px tile + 2 px gap
    let mut rows: Vec<Vec<&spectrum_app::widgets::Widget>> = Vec::new();
    for tile in tiles {
        match rows.last_mut() {
            Some(row) if (row[0].rect.min.y - tile.rect.min.y).abs() < 1.0 => row.push(tile),
            _ => rows.push(vec![tile]),
        }
    }
    assert!(
        rows.len() >= 2,
        "twelve tiles must wrap into more than one row, got {} row(s)",
        rows.len()
    );
    for row in &rows {
        for pair in row.windows(2) {
            let dx = pair[1].rect.min.x - pair[0].rect.min.x;
            assert!(
                (dx - pitch).abs() < 0.6,
                "pitch in a row is {dx:.1} px, want {pitch}: {:?}",
                row.iter().map(|w| w.rect.min.x).collect::<Vec<_>>()
            );
        }
        let right = row.iter().map(|w| w.rect.max.x).fold(0.0f32, f32::max);
        assert!(
            right <= scroll.rect.max.x + 0.5,
            "a row reaches x={right:.1} past the grid at x={}",
            scroll.rect.max.x
        );
    }
    // The design's Main frame is 1207 px wide with 20 px side padding: 1167 px of grid = seven
    // 166 px columns. Ours is 1440 - 232 sidebar - 40 padding = 1168.
    // No whole column of slack: the grid must fill its width, whatever that width is.
    let used = rows[0].len() as f32 * pitch;
    assert!(
        used >= scroll.rect.width() - pitch + 1.0,
        "{} columns use {used:.0} px of a {} px grid — a column is left empty",
        rows[0].len(),
        scroll.rect.width()
    );
}

/// The design's grid is seven `164 px + 2 px` columns inside `Main` (1207 px frame, 20 px padding).
/// `egui_kittest` wraps the app in an 8 px window margin, so the harness screen is narrower than
/// the real window; this checks the column count against the design's own widths instead.
#[test]
fn grid_columns_match_the_design_widths() {
    assert_eq!(spectrum_app::app::columns_for_width(1167.0), 7);
    assert_eq!(spectrum_app::app::columns_for_width(1168.0), 7);
    assert_eq!(spectrum_app::app::columns_for_width(1152.0), 6);
    assert_eq!(spectrum_app::app::columns_for_width(900.0), 5);
    assert_eq!(
        spectrum_app::app::columns_for_width(120.0),
        1,
        "a narrow column still gets one tile"
    );
}
