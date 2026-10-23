//! The "Add Folder" flow: browse a directory, pick it, and it joins the library.
//!
//! Every mutation here goes through `folder.add`, so a rejected pick leaves the library exactly as
//! it was and the window stays open with the reason — which is what these tests check, along with
//! the ids the browser promises (`docs/ui-spec.md`, ```ids-on-browser`).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use egui::DroppedFile;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use spectrum_app::app::AppState;
use spectrum_testkit::fixture;

fn base(tag: &str) -> PathBuf {
    let mut root = std::env::temp_dir();
    root.push(format!("spectrum-egui-addfolder-{tag}"));
    std::fs::create_dir_all(&root).unwrap_or_default();
    root
}

/// A library folder with two photos, and a sibling folder with three that is not in the library yet.
struct Tree {
    /// In the library.
    library: PathBuf,
    /// Not in the library: this is what the browser should offer and add.
    new: PathBuf,
    /// A directory with nothing media-shaped in it.
    empty: PathBuf,
}

fn tree(tag: &str) -> Tree {
    let root = base(tag);
    let library = root.join("library");
    let new = root.join("albums");
    let empty = root.join("empty");
    std::fs::create_dir_all(&library).unwrap_or_default();
    std::fs::create_dir_all(&new).unwrap_or_default();
    std::fs::create_dir_all(&empty).unwrap_or_default();
    for i in 0..2 {
        fixture::write_png_fixture(&library, &format!("a{i}.png"), 20, 20, [200, 40, 40]);
    }
    for i in 0..3 {
        fixture::write_png_fixture(&new, &format!("b{i}.png"), 20, 20, [40, 200, 40]);
    }
    fixture::write_png_fixture(&root, "loose.png", 20, 20, [10, 10, 10]);
    Tree {
        library,
        new,
        empty,
    }
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

fn click(harness: &mut Harness<'static>, label: &str) {
    let node = harness
        .root()
        .query_by_label(label)
        .unwrap_or_else(|| panic!("nothing in the AccessKit tree is labelled {label:?}"));
    node.click();
}

fn folder_names(state: &AppState) -> Vec<String> {
    state
        .lib
        .folders
        .iter()
        .map(|slot| {
            slot.path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
        })
        .collect::<Option<Vec<_>>>()
        .unwrap_or_default()
}

/// The same spec reader the registry tests use, for the browser's own checklist block.
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

/// A dropped path, so a drop can be driven without an OS file drag.
#[derive(Debug)]
struct DroppedPath {
    path: PathBuf,
}

impl DroppedFile for DroppedPath {
    fn path(&self) -> &Path {
        &self.path
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.path).map_err(|err| err.to_string())
    }
}

#[test]
fn browse_opens_the_browser_on_the_directory_above_the_library() {
    let t = tree("open");
    let state = Rc::new(RefCell::new(AppState::new(t.library.clone())));
    let mut h = harness(state.clone());
    h.run();

    assert!(state.borrow().browser_path().is_none(), "closed to start");
    // Nothing typed: the button picks instead of doing nothing.
    click(&mut h, "Add folder");
    h.run();

    assert_eq!(
        state.borrow().browser_path(),
        Some(
            &t.library
                .parent()
                .map(PathBuf::from)
                .expect("fixture has a parent")
        ),
        "the browser starts one level above the folder being viewed"
    );
    let drawn = ids(&state.borrow());
    for id in promised_ids("ids-on-browser") {
        assert!(
            drawn.iter().any(|have| have.starts_with(id.as_str())),
            "promised browser id {id} is missing: {drawn:?}"
        );
    }
    assert!(
        drawn.iter().any(|id| id == "dir:albums"),
        "the sibling folder is offered: {drawn:?}"
    );
    assert!(
        !drawn.iter().any(|id| id.starts_with("dir:loose")),
        "a file is never offered as a folder"
    );
}

#[test]
fn a_typed_path_is_added_by_the_same_button_and_opens_no_browser() {
    let t = tree("typed");
    let state = Rc::new(RefCell::new(AppState::new(t.library.clone())));
    state.borrow_mut().add_path = t.new.display().to_string();
    let mut h = harness(state.clone());
    h.run();

    click(&mut h, "Add folder");
    h.run();

    assert!(
        state.borrow().browser_path().is_none(),
        "a typed path is added directly: no window appears"
    );
    assert_eq!(
        folder_names(&state.borrow()),
        vec!["library".to_string(), "albums".to_string()],
        "the typed folder joined the list"
    );
    assert!(state.borrow().add_path.is_empty(), "and the field cleared");
}

#[test]
fn walking_in_and_pressing_scan_adds_the_folder() {
    let t = tree("scan");
    let state = Rc::new(RefCell::new(AppState::new(t.library.clone())));
    let mut store = base("scan");
    store.push("library.json");
    state.borrow_mut().set_store(Some(store.clone()));
    state
        .borrow_mut()
        .open_browser_at(t.library.parent().expect("parent").to_path_buf());
    let mut h = harness(state.clone());
    h.run();

    click(&mut h, "albums  ·  3");
    h.run();
    assert_eq!(
        state.borrow().browser_path(),
        Some(&t.new),
        "clicking a row walks into it; the count tells you which folder to pick"
    );

    click(&mut h, "Scan");
    h.run();

    assert!(
        state.borrow().browser_path().is_none(),
        "a pick closes the window"
    );
    assert_eq!(
        folder_names(&state.borrow()),
        vec!["library".to_string(), "albums".to_string()],
        "the picked folder joins the list"
    );
    assert_eq!(
        state.borrow().lib.folder(),
        t.new,
        "and becomes the folder being viewed"
    );
    assert_eq!(
        state.borrow().lib.entries.len(),
        3,
        "its photos are scanned"
    );
    let saved = std::fs::read_to_string(&store).unwrap_or_default();
    assert!(
        saved.contains("albums"),
        "the list is persisted so a restart keeps it: {saved}"
    );
}

#[test]
fn cancel_changes_nothing() {
    let t = tree("cancel");
    let state = Rc::new(RefCell::new(AppState::new(t.library.clone())));
    let before = state.borrow().lib.entries.len();
    state.borrow_mut().open_browser_at(t.new.clone());
    let mut h = harness(state.clone());
    h.run();

    click(&mut h, "Cancel");
    h.run();

    assert!(state.borrow().browser_path().is_none());
    assert_eq!(folder_names(&state.borrow()), vec!["library".to_string()]);
    assert_eq!(
        state.borrow().lib.entries.len(),
        before,
        "nothing was scanned"
    );
}

#[test]
fn a_duplicate_reports_why_and_stays_open() {
    let t = tree("duplicate");
    let state = Rc::new(RefCell::new(AppState::new(t.library.clone())));
    // The browser showing the folder that is already in the library.
    state.borrow_mut().open_browser_at(t.library.clone());
    let mut h = harness(state.clone());
    h.run();

    click(&mut h, "Scan");
    h.run();

    let borrowed = state.borrow();
    assert!(
        borrowed
            .notice_text()
            .is_some_and(|n| n.contains("already in the library")),
        "the reason must be visible: {:?}",
        borrowed.notice_text()
    );
    assert!(
        borrowed.browser_path().is_some(),
        "the window stays open when the pick is refused"
    );
    assert_eq!(folder_names(&borrowed), vec!["library".to_string()]);
}

#[test]
fn a_directory_that_cannot_be_read_says_so() {
    let t = tree("unreadable");
    let state = Rc::new(RefCell::new(AppState::new(t.library.clone())));
    state
        .borrow_mut()
        .open_browser_at(t.library.join("no-such-folder"));
    let mut h = harness(state.clone());
    h.run();

    let borrowed = state.borrow();
    assert!(
        borrowed.notice_text().is_none(),
        "listing a directory is not a command, so no notice is raised"
    );
    let drawn = ids(&borrowed);
    assert!(
        drawn.iter().any(|id| id == "info:browser-error"),
        "the window itself carries the reason: {drawn:?}"
    );
    let text = harness_root_text(&mut h);
    assert!(
        text.iter().any(|t| t.contains("cannot read")),
        "the error text names the problem: {text:?}"
    );
}

/// Every string the tree announces, for checking painted text without matching pixels.
fn harness_root_text(harness: &mut Harness<'static>) -> Vec<String> {
    harness
        .root()
        .children_recursive()
        .map(|node| {
            let node = node.accesskit_node();
            node.label()
                .or(node.value())
                .unwrap_or_default()
                .to_string()
        })
        .filter(|text| !text.is_empty())
        .collect()
}

#[test]
fn an_empty_directory_says_so_instead_of_showing_nothing() {
    let t = tree("empty");
    let state = Rc::new(RefCell::new(AppState::new(t.library.clone())));
    state.borrow_mut().open_browser_at(t.empty.clone());
    let mut h = harness(state.clone());
    h.run();

    let announced = harness_root_text(&mut h);
    assert!(
        announced.iter().any(|text| text == "No folders inside"),
        "an empty directory must say it is empty rather than render a blank list: {announced:?}"
    );
    let drawn = ids(&state.borrow());
    assert!(
        !drawn.iter().any(|id| id.starts_with("dir:")),
        "there is nothing to offer: {drawn:?}"
    );
    // Scan still means "add this directory": an empty folder is a legitimate thing to add.
    click(&mut h, "Scan");
    h.run();
    assert_eq!(
        folder_names(&state.borrow()),
        vec!["library".to_string(), "empty".to_string()],
        "adding an empty folder is allowed; it just has nothing in it yet"
    );
    assert_eq!(
        state.borrow().lib.entries.len(),
        0,
        "and the scan finds nothing"
    );
}

#[test]
fn escape_backs_out_of_the_browser_without_clearing_the_selection() {
    let t = tree("escape");
    let state = Rc::new(RefCell::new(AppState::new(t.library.clone())));
    state
        .borrow_mut()
        .run("grid.click", serde_json::json!({ "index": 0 }));
    state.borrow_mut().open_browser_at(t.new.clone());
    let mut h = harness(state.clone());
    h.run();

    h.key_press(egui::Key::Escape);
    h.run();

    assert!(state.borrow().browser_path().is_none(), "Escape closed it");
    assert_eq!(
        state.borrow().lib.selection.set.len(),
        1,
        "Escape backs out of the topmost thing only, so the selection survives"
    );
}

#[test]
fn enter_confirms_the_shown_directory() {
    let t = tree("enter");
    let state = Rc::new(RefCell::new(AppState::new(t.library.clone())));
    state.borrow_mut().open_browser_at(t.new.clone());
    let mut h = harness(state.clone());
    h.run();

    h.key_press(egui::Key::Enter);
    h.run();

    assert_eq!(
        folder_names(&state.borrow()),
        vec!["library".to_string(), "albums".to_string()],
        "Enter is the keyboard path to Scan"
    );
    assert_eq!(state.borrow().lib.entries.len(), 3);
}

#[test]
fn dropping_a_folder_adds_it_and_dropping_a_file_does_not() {
    let t = tree("drop");
    let state = Rc::new(RefCell::new(AppState::new(t.library.clone())));
    let mut h = harness(state.clone());
    h.run();

    h.input_mut()
        .dropped_files
        .push(std::sync::Arc::new(DroppedPath {
            path: t.new.clone(),
        }));
    h.run();
    assert_eq!(
        folder_names(&state.borrow()),
        vec!["library".to_string(), "albums".to_string()],
        "a folder dropped on the window joins the library"
    );

    h.input_mut()
        .dropped_files
        .push(std::sync::Arc::new(DroppedPath {
            path: t.library.join("a0.png"),
        }));
    h.run();
    assert_eq!(
        folder_names(&state.borrow()),
        vec!["library".to_string(), "albums".to_string()],
        "a loose file is not a folder"
    );
    assert!(
        state.borrow().notice_text().is_none(),
        "ignoring a file is not an error worth reporting"
    );
}

#[test]
fn browser_ids_only_exist_while_the_browser_is_open() {
    let t = tree("registry");
    let state = Rc::new(RefCell::new(AppState::new(t.library.clone())));
    let mut h = harness(state.clone());
    h.run();

    let closed = ids(&state.borrow());
    assert!(
        closed.iter().any(|id| id == "button:add-folder"),
        "the button that opens it is always on screen: {closed:?}"
    );
    for prefix in ["dir:", "button:browser-", "panel:browser", "browser:scroll"] {
        assert!(
            !closed.iter().any(|id| id.starts_with(prefix)),
            "{prefix} ids must not be clickable while the window is closed"
        );
    }

    click(&mut h, "Add folder");
    h.run();
    let open = ids(&state.borrow());
    assert!(
        open.iter().any(|id| id == "panel:browser")
            && open.iter().any(|id| id == "button:browser-scan"),
        "opening it registers them: {open:?}"
    );

    click(&mut h, "Cancel");
    h.run();
    let closed_again = ids(&state.borrow());
    assert!(
        !closed_again
            .iter()
            .any(|id| id.starts_with("button:browser-")),
        "closing removes them again"
    );
}
