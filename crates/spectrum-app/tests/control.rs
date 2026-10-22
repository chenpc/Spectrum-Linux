//! The control channel, driven without a window or a socket: `control::handle` is the same
//! function the TCP server calls, so these tests prove the IPC path and the UI share one surface.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;

use egui_kittest::Harness;
use serde_json::{json, Value};
use spectrum_app::app::AppState;
use spectrum_app::control::{self, Request};
use spectrum_testkit::fixture;

const BT2020: u16 = 9;
const SRGB: u16 = 13;
const BT709: u16 = 1;
const HLG: u16 = 18;

fn fixture_dir(tag: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("spectrum-egui-ctrl-{tag}"));
    std::fs::create_dir_all(&dir).unwrap_or_default();
    fixture::write_fixture(&dir, "hlg_photo.heic", BT2020, HLG, 0, true);
    fixture::write_fixture(&dir, "sdr_photo.heic", BT709, SRGB, 0, false);
    dir
}

fn harness(state: Rc<RefCell<AppState>>) -> Harness<'static> {
    Harness::builder()
        .with_size(egui::vec2(1440.0, 900.0))
        .with_theme(egui::Theme::Light)
        .build_ui(move |ui| state.borrow_mut().render(ui))
}

/// Feed whatever the control channel queued into this pass's raw input — the same thing
/// `raw_input_hook` does in the window — then draw.
fn pump(state: &Rc<RefCell<AppState>>, harness: &mut Harness<'static>) {
    let mut batch = Vec::new();
    while let Some(events) = Some(state.borrow_mut().take_events()).filter(|e| !e.is_empty()) {
        batch.extend(events);
        if batch.len() > 8 {
            break;
        }
    }
    harness.input_mut().events.append(&mut batch);
    harness.run();
}

/// Ask the app one thing with a drawn context available, as the window's server thread does.
fn ask_ctx(
    state: &Rc<RefCell<AppState>>,
    ctx: &egui::Context,
    method: &str,
    params: Value,
) -> Value {
    let (tx, _rx) = mpsc::channel();
    let req = Request {
        id: json!(1),
        method: method.to_string(),
        params,
        reply: tx,
    };
    control::handle(&mut state.borrow_mut(), Some(ctx), &req)
}

/// Ask the app one thing, as a client would. The reply arrives here instead of on a socket; the
/// queued events still travel through `AppState::logic`, exactly as they do in the window.
fn ask(state: &Rc<RefCell<AppState>>, method: &str, params: Value) -> Value {
    let (tx, _rx) = mpsc::channel();
    let req = Request {
        id: json!(1),
        method: method.to_string(),
        params,
        reply: tx,
    };
    control::handle(&mut state.borrow_mut(), None, &req)
}

#[test]
fn inspect_reports_the_frame_and_what_the_screen_holds() {
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir("inspect"))));
    let mut harness = harness(state.clone());
    harness.run();

    let reply = ask(&state, "ui.inspect", json!({}));
    assert_eq!(reply["ok"], json!(true), "{reply}");
    assert!(
        reply["frame"].as_u64().unwrap_or(0) > 0,
        "frames are counted"
    );
    assert!(reply["widgets"].as_u64().unwrap_or(0) > 30, "{reply}");
    assert_eq!(
        reply["model"]["entries"],
        json!(2),
        "both fixtures are listed"
    );
    assert_eq!(reply["model"]["view"], json!("all"));
    assert_eq!(
        reply["id"],
        json!(1),
        "the request id comes back so a client can match replies"
    );
}

#[test]
fn clicking_a_tile_by_id_selects_it() {
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir("click"))));
    let mut harness = harness(state.clone());
    harness.run();

    let reply = ask(&state, "ui.click", json!({"id": "tile:hlg_photo.heic"}));
    assert_eq!(reply["ok"], json!(true), "{reply}");
    let rect = reply["rect"].as_array().cloned().unwrap_or_default();
    assert_eq!(rect.len(), 4, "the reply reports where it clicked: {reply}");

    pump(&state, &mut harness);
    let selected = state.borrow().lib.selection.count();
    assert_eq!(selected, 1, "one tile selected after the release frame");
    let name = state
        .borrow()
        .lib
        .sole()
        .map(|i| state.borrow().lib.entries[i].name());
    assert_eq!(name.as_deref(), Some("hlg_photo.heic"), "the right tile");
}

#[test]
fn clicking_an_id_that_is_not_there_names_what_it_looked_for() {
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir("missing"))));
    let mut harness = harness(state.clone());
    harness.run();

    let reply = ask(&state, "ui.click", json!({"id": "tile:nope.heic"}));
    assert_eq!(reply["ok"], json!(false), "{reply}");
    let error = reply["error"].as_str().unwrap_or_default();
    assert!(
        error.contains("no widget tile:nope.heic on screen"),
        "a miss must be reported, not turned into a coordinate click: {error}"
    );
    assert_eq!(
        state.borrow().lib.selection.count(),
        0,
        "a rejected request changes nothing"
    );
}

#[test]
fn commands_and_the_ui_go_through_the_same_registry() {
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir("command"))));
    let mut harness = harness(state.clone());
    harness.run();

    let reply = ask(
        &state,
        "engine.execute",
        json!({"command": "filter.kind", "params": {"kind": "video"}}),
    );
    assert_eq!(reply["ok"], json!(true), "{reply}");
    assert_eq!(reply["ran"], json!(true), "the command ran");
    assert_eq!(reply["model"]["filter"]["kind"], json!("video"));

    let video_only = ask(
        &state,
        "engine.execute",
        json!({"command": "view.set", "params": {"view": "months"}}),
    );
    assert_eq!(video_only["model"]["view"], json!("months"));

    let bad = ask(
        &state,
        "engine.execute",
        json!({"command": "folder.add", "params": {"path": "/no/such/album"}}),
    );
    assert_eq!(
        bad["ran"],
        json!(false),
        "a rejected command reports ran=false"
    );
    assert!(
        bad["notice"]
            .as_str()
            .unwrap_or_default()
            .contains("/no/such/album"),
        "the reason is in the reply: {bad}"
    );
    assert_eq!(
        bad["model"]["entries"],
        json!(2),
        "and the model is unchanged"
    );

    let typo = ask(&state, "engine.execute", json!({"params": {}}));
    assert_eq!(
        typo["ok"],
        json!(false),
        "a request without a command is refused: {typo}"
    );
}

#[test]
fn a_widget_filter_returns_only_matching_ids() {
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir("filter"))));
    let mut harness = harness(state.clone());
    harness.run();

    let reply = ask(&state, "ui.widgets", json!({"filter": "segment:"}));
    let list = reply["widgets"].as_array().cloned().unwrap_or_default();
    assert!(!list.is_empty(), "segments exist: {reply}");
    for widget in &list {
        let id = widget["id"].as_str().unwrap_or_default();
        assert!(id.starts_with("segment:"), "filter leaked {id}");
    }
    assert_eq!(reply["count"], json!(list.len()), "count matches the list");
}

#[test]
fn typing_and_enter_submit_the_folder_path() {
    // A second folder: `AppState::new` already holds the first, and re-adding it is a rejection
    // (`BadParam`), which would leave the text in the field for the wrong reason.
    let dir = fixture_dir("typing-b");
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir("typing-a"))));
    let mut harness = harness(state.clone());
    harness.run();

    ask(&state, "ui.click", json!({"id": "field:add-path"}));
    pump(&state, &mut harness);
    ask(
        &state,
        "ui.type",
        json!({"text": dir.display().to_string()}),
    );
    pump(&state, &mut harness);
    ask(&state, "ui.key", json!({"key": "enter"}));
    pump(&state, &mut harness);

    let folders: Vec<String> = state
        .borrow()
        .lib
        .folders
        .iter()
        .map(|slot| slot.label())
        .collect();
    assert_eq!(
        folders.len(),
        2,
        "Enter submits the only field on screen: {folders:?}"
    );
    assert!(
        state.borrow().add_path.is_empty(),
        "the field clears after a submit"
    );
}

#[test]
fn the_app_can_photograph_itself() {
    let state = Rc::new(RefCell::new(AppState::new(fixture_dir("shot"))));
    let mut harness = harness(state.clone());
    harness.run();

    let path = std::env::temp_dir().join("spectrum-egui-shot.bmp");
    std::fs::remove_file(&path).ok();
    let reply = ask_ctx(
        &state,
        &harness.ctx,
        "ui.screenshot",
        json!({"path": path.display().to_string()}),
    );
    assert_eq!(reply["ok"], json!(true), "{reply}");

    // egui answers the command with a `Screenshot` event a frame or two later.
    harness.run_steps(8);

    let bytes = std::fs::read(&path).unwrap_or_default();
    assert!(
        bytes.len() > 54,
        "a BMP header plus pixels: {} bytes",
        bytes.len()
    );
    assert_eq!(&bytes[..2], b"BM", "a bitmap, not a text file");
    let width = u32::from_le_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]) as usize;
    let height = u32::from_le_bytes([bytes[22], bytes[23], bytes[24], bytes[25]]) as usize;
    assert_eq!(
        (width, height),
        (1440, 900),
        "the capture is the window the harness drew"
    );
    assert!(
        state
            .borrow()
            .shots
            .iter()
            .any(|shot| shot.contains("1440×900")),
        "{:?}",
        state.borrow().shots
    );
}
