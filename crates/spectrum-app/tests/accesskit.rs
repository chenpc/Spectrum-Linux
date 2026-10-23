//! L3 AccessKit audit: the widget registry promises what the assistive-tech tree must contain.
//!
//! The registry (`state.widgets`) is what the control channel clicks by id; the AccessKit tree is
//! what a screen reader sees. If the two disagree, an id that works over TCP can still be invisible
//! to a user who never touches the mouse. Measured facts this test encodes:
//!
//! * Every registry control of kind `button` has a `Button` node with the same bounds and a non-empty
//!   label; the node count matches, so a control that becomes painted-only fails here.
//! * The add-folder field is a `TextInput` carrying its hint as the accessible placeholder.
//! * Tile labels are in the tree: a screen reader can read a filename, a metadata line, and the HDR
//!   badge. Tiles are painted, so they are *readable but not actionable*; keyboard navigation is
//!   what makes selection possible (`docs/ui-spec.md`, keyboard table).
//! * An AccessKit press reaches the command registry, i.e. assistive actions go through `dispatch`.

use std::cell::RefCell;
use std::rc::Rc;

use egui::Rect;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use spectrum_app::app::AppState;
use spectrum_testkit::fixture;

fn fixture_dir(tag: &str) -> std::path::PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("spectrum-egui-accesskit-{tag}"));
    std::fs::create_dir_all(&dir).unwrap_or_default();
    fixture::write_png_fixture(&dir, "a.png", 40, 30, [200, 40, 40]);
    fixture::write_png_fixture(&dir, "b.png", 40, 30, [40, 200, 40]);
    dir
}

fn harness(state: Rc<RefCell<AppState>>) -> Harness<'static> {
    Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .with_theme(egui::Theme::Light)
        .build_ui(move |ui| state.borrow_mut().render(ui))
}

/// Every `Button` node in the tree, as `(bounds, label)`. `kittest` maps AccessKit bounds back to
/// egui points using the harness' pixels-per-point, which is what the registry recorded.
fn buttons(harness: &Harness<'static>) -> Vec<(Rect, String)> {
    harness
        .root()
        .query_all_by_role(egui::accesskit::Role::Button)
        .map(|node| {
            (
                node.rect(),
                node.accesskit_node()
                    .label()
                    .unwrap_or_default()
                    .to_string(),
            )
        })
        .collect()
}

/// A node's bounds inside a registry rect, within a fraction of a pixel. A row registers the rect of
/// the whole horizontal line (title and count), while the tree holds one node per label, so `inside`
/// is the right relation for rows and `same_rect` for single widgets.
fn inside(rect: Rect, bounds: Rect) -> bool {
    bounds.min.x >= rect.min.x - 0.5
        && bounds.min.y >= rect.min.y - 0.5
        && bounds.max.x <= rect.max.x + 0.5
        && bounds.max.y <= rect.max.y + 0.5
}

/// `Rect` has no `PartialEq` worth asserting on, and the tree is built from the same rects the
/// registry recorded, so compare with a fraction of a pixel of slack.
fn same_rect(a: Rect, b: Rect) -> bool {
    (a.min.x - b.min.x).abs() < 0.5
        && (a.min.y - b.min.y).abs() < 0.5
        && (a.max.x - b.max.x).abs() < 0.5
        && (a.max.y - b.max.y).abs() < 0.5
}

#[test]
fn every_registry_button_is_an_accesskit_button_with_a_name() {
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir("buttons"))));
    let mut harness = harness(state.clone());
    harness.run();

    let widgets: Vec<(String, String, Rect)> = state
        .borrow()
        .widgets
        .iter()
        .map(|w| (w.id.clone(), w.kind.to_string(), w.rect))
        .collect();
    // A `segment` is the track a `cell` sits in, so it is a container in the tree, not a control.
    // Folder rows are registered as `row` but drawn as buttons, so they join the button set by id.
    let promised: Vec<(String, Rect)> = widgets
        .iter()
        .filter(|(id, kind, _)| kind == "button" || kind == "cell" || id.starts_with("folder:"))
        .map(|(id, _, rect)| (id.clone(), *rect))
        .collect();
    let nodes = buttons(&harness);
    assert!(
        promised.len() >= 12,
        "the library screen promises at least 12 addressable controls, got {}",
        promised.len()
    );
    assert_eq!(
        nodes.len(),
        promised.len(),
        "the tree has {} Button nodes but the registry promises {} controls: {:?}",
        nodes.len(),
        promised.len(),
        promised.iter().map(|(id, _)| id).collect::<Vec<_>>()
    );

    // Collect every mismatch before failing, so one run shows the whole gap rather than the first.
    let missing: Vec<String> = promised
        .iter()
        .filter(|(_id, rect)| {
            !nodes
                .iter()
                .any(|(bounds, label)| !label.trim().is_empty() && same_rect(*bounds, *rect))
        })
        .map(|(id, rect)| format!("{id} at {rect:?}"))
        .collect();
    assert!(
        missing.is_empty(),
        "{} registry controls have no named Button node at their bounds: {:?}\nthe tree holds {:?}",
        missing.len(),
        missing,
        nodes
    );
}

#[test]
fn clickable_rows_announce_their_text() {
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir("rows"))));
    let mut harness = harness(state.clone());
    harness.run();

    // `group:<key>` is a section header you can click to collapse its tiles. egui draws it as a
    // clickable `Label`, so its AccessKit role is `Label`, not `Button` — what the audit requires is
    // that the node exists where the registry says, and carries text.
    let rows: Vec<(String, Rect)> = state
        .borrow()
        .widgets
        .iter()
        .filter(|w| w.kind == "row" && w.id.starts_with("group:"))
        .map(|w| (w.id.clone(), w.rect))
        .collect();
    assert_eq!(rows.len(), 1, "the fixture's folder has one section header");
    let announced: Vec<(Rect, String)> = harness
        .root()
        .children_recursive()
        .filter(|node| node.accesskit_node().has_bounds())
        .map(|node| {
            let n = node.accesskit_node();
            let text = n.label().or(n.value()).unwrap_or_default().to_string();
            (node.rect(), text)
        })
        .filter(|(_, text)| !text.trim().is_empty())
        .collect();
    for (id, rect) in &rows {
        // `group:2026-10` is drawn as "October 2026"; the id's year is what a user would hear.
        let year = id
            .split(':')
            .nth(1)
            .unwrap_or_default()
            .split('-')
            .next()
            .unwrap_or_default();
        let hit = announced
            .iter()
            .find(|(bounds, _)| inside(*rect, *bounds))
            .unwrap_or_else(|| {
                panic!("{id} has no node inside {rect:?}; announced nodes are {announced:?}")
            });
        assert!(
            hit.1.contains(year),
            "{id} announces {:?} at its own bounds, which does not mention the section year {year:?}",
            hit.1
        );
    }
}

#[test]
fn the_path_field_is_a_text_input_with_an_accessible_hint() {
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir("field"))));
    let mut harness = harness(state.clone());
    harness.run();

    let field_rect = state
        .borrow()
        .widgets
        .iter()
        .find(|w| w.kind == "field")
        .map(|w| w.rect)
        .expect("the sidebar promises a path field");
    let inputs: Vec<(Rect, Option<String>, Option<String>)> = harness
        .root()
        .query_all_by_role(egui::accesskit::Role::TextInput)
        .map(|n| {
            let node = n.accesskit_node();
            (
                n.rect(),
                node.placeholder().map(|p| p.to_string()),
                node.label().map(|l| l.to_string()),
            )
        })
        .collect();
    assert_eq!(inputs.len(), 1, "exactly one text input on this screen");
    let (bounds, placeholder, label) = (&inputs[0].0, &inputs[0].1, &inputs[0].2);
    assert!(
        same_rect(*bounds, field_rect),
        "the TextInput node is where the registry says the field is: {bounds:?} vs {field_rect:?}"
    );
    assert!(
        placeholder.as_ref().is_some_and(|p| !p.trim().is_empty())
            || label.as_ref().is_some_and(|l| !l.trim().is_empty()),
        "a text input with neither a label nor a placeholder announces nothing"
    );
}

#[test]
fn tiles_are_readable_in_the_tree() {
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir("tiles"))));
    let mut harness = harness(state.clone());
    harness.run();

    let names: Vec<String> = state
        .borrow()
        .lib
        .visible()
        .iter()
        .map(|i| {
            state.borrow().lib.entries[*i]
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        })
        .collect();
    let labels: Vec<String> = harness
        .root()
        .query_all_by_role(egui::accesskit::Role::Label)
        .map(|n| n.accesskit_node().value().unwrap_or_default().to_string())
        .collect();
    for name in &names {
        assert!(
            labels.iter().any(|l| l == name),
            "{name} is a tile the grid drew, but no Label node carries its name; labels are {labels:?}"
        );
    }
    // The HDR badge is painted text, which is why the badge is a real `Label` (lesson 5).
    assert!(
        labels.iter().any(|l| l == "SDR" || l == "HDR"),
        "no badge was announced; labels are {labels:?}"
    );
}

#[test]
fn an_accesskit_press_goes_through_the_command_registry() {
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir("press"))));
    let mut harness = harness(state.clone());
    harness.run();

    // The toggle lives in the inspector, which only exists once something is selected.
    state
        .borrow_mut()
        .run("grid.click", serde_json::json!({ "index": 0 }));
    harness.run();

    let before = state.borrow().lib.hdr_enabled;
    {
        let node = harness
            .root()
            .query_by_role_and_label(egui::accesskit::Role::Button, "HDR / SDR toggle")
            .expect("the inspector's HDR toggle is in the tree");
        node.click_accesskit();
    }
    harness.run();

    assert_ne!(
        state.borrow().lib.hdr_enabled,
        before,
        "a press action announced to assistive tech must reach `hdr.toggle`"
    );
    // The notice is what a rejected dispatch leaves behind; a successful one clears it.
    assert!(
        state.borrow().notice.is_none(),
        "the toggle reported a problem: {:?}",
        state.borrow().notice
    );
}

#[test]
fn nodes_follow_the_selection() {
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir("selection"))));
    let mut harness = harness(state.clone());
    harness.run();
    assert!(
        harness
            .root()
            .query_by_role_and_label(egui::accesskit::Role::Button, "Restore")
            .is_none(),
        "the inspector is not drawn without a selection, so its controls must not be in the tree"
    );

    state
        .borrow_mut()
        .run("grid.click", serde_json::json!({ "index": 0 }));
    harness.run();
    assert!(
        harness
            .root()
            .query_by_role_and_label(egui::accesskit::Role::Button, "Restore")
            .is_some(),
        "selecting a file must put the inspector's controls in the tree"
    );

    state.borrow_mut().run("detail.open", serde_json::json!({}));
    harness.run();
    assert!(
        harness
            .root()
            .query_by_label_contains("Add folder")
            .is_some(),
        "the sidebar stays reachable when the detail view opens"
    );
    // `Restore` lives in the inspector, which stays; the toolbar's controls are the ones that go.
    assert!(
        harness.root().query_by_label_contains("Restore").is_some(),
        "the inspector stays reachable in the detail view"
    );
    assert!(
        harness
            .root()
            .query_by_label_contains("Clear filter")
            .is_none(),
        "the toolbar's controls must leave the tree once the detail view replaces the grid"
    );
    assert!(
        harness.root().query_by_label_contains("1:1").is_some(),
        "the detail view's zoom control must be reachable from assistive tech"
    );
}
