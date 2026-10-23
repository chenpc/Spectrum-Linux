#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

//! The control channel: JSON-lines IPC, the same path tests, scripts and agents use.
//!
//! One request per line, one reply per line:
//! `{"id":1,"method":"ui.click","params":{"id":"segment:view:Months"}}` →
//! `{"id":1,"ok":true,"rect":[232,52,118,26]}`.
//!
//! Every request goes through the app's own public surface: widget ids from the registry
//! ([`crate::widgets`]) and commands through [`spectrum_core::dispatch`]. Nothing here reads
//! private state and nothing here synthesises OS input. A request that cannot be answered names
//! what it looked for (`no widget <id> on screen (see ui.widgets)`) instead of clicking a guess.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::time::{Duration, Instant};

use egui::{Context, Event, Key, Modifiers, MouseWheelUnit, Rect, TouchPhase};
use serde_json::{json, Value};

use crate::app::AppState;

/// A request from a client, with the channel its reply goes back on.
pub struct Request {
    pub id: Value,
    pub method: String,
    pub params: Value,
    pub reply: std::sync::mpsc::Sender<Value>,
}

/// The app's end of the channel: requests to answer during [`crate::app::AppState::logic`].
pub struct Inbox {
    rx: Receiver<Request>,
}

/// A client's end of the channel: the TCP server thread, or a test.
#[derive(Clone)]
pub struct Handle {
    tx: SyncSender<Request>,
}

/// A bounded channel pair; `capacity` is how many unanswered requests may queue up.
pub fn channel(capacity: usize) -> (Handle, Inbox) {
    let (tx, rx) = mpsc::sync_channel(capacity.max(1));
    (Handle { tx }, Inbox { rx })
}

impl Inbox {
    /// Requests that arrived since the last frame. Answering them never blocks a frame.
    pub fn drain(&mut self) -> Vec<Request> {
        let mut out = Vec::new();
        while let Ok(req) = self.rx.try_recv() {
            out.push(req);
        }
        out
    }
}

impl Handle {
    /// Send a request and wait for the app's reply. `None` once the app is gone.
    pub fn request(&self, id: Value, method: &str, params: Value) -> Option<Value> {
        self.send(id, method, params)?.recv().ok()
    }

    /// Send a request, calling `nudge` (usually `Context::request_repaint`) until a frame answers
    /// it, then give up with an honest error. `eframe` stops drawing while the window is hidden,
    /// and a reply is only ever produced inside a frame — a silent hang would be worse than a
    /// message saying why.
    pub fn request_nudged(
        &self,
        id: Value,
        method: &str,
        params: Value,
        nudge: &dyn Fn(),
        timeout: Duration,
    ) -> Value {
        let Some(rx) = self.send(id, method, params) else {
            return json!({"ok": false, "error": "the app is gone"});
        };
        let deadline = Instant::now() + timeout;
        loop {
            nudge();
            match rx.recv_timeout(Duration::from_millis(150)) {
                Ok(reply) => return reply,
                Err(RecvTimeoutError::Disconnected) => {
                    return json!({"ok": false, "error": "the app has stopped"});
                }
                Err(RecvTimeoutError::Timeout) => {
                    if Instant::now() >= deadline {
                        return json!({"ok": false, "error": format!("no frame answered the request within {}s — the window may be hidden or minimized", timeout.as_secs())});
                    }
                }
            }
        }
    }

    fn send(&self, id: Value, method: &str, params: Value) -> Option<Receiver<Value>> {
        let (tx, rx) = mpsc::channel();
        self.tx
            .send(Request {
                id,
                method: method.to_string(),
                params,
                reply: tx,
            })
            .ok()?;
        Some(rx)
    }
}

/// Answer one request against the live app and return the reply (with the request's `id` echoed).
///
/// `ctx` is `None` in a test that has not drawn a frame yet; registry-backed methods then report
/// "no frame drawn yet" rather than guessing at geometry.
pub fn handle(app: &mut AppState, ctx: Option<&Context>, req: &Request) -> Value {
    let result = match req.method.as_str() {
        "ui.inspect" => ok(json!({
            "frame": app.frame,
            // Logic ticks versus paint passes. `drawn < frame` means the window manager stopped
            // asking for draws, so clicks cannot register.
            "drawn": app.drawn,
            "drawing": app.drawn == app.frame,
            "widgets": app.widgets.len(),
            "screen": ctx.map(|c| {
                let r = c.viewport_rect();
                [r.width(), r.height()]
            }),
            "shots": app.shots.clone(),
            "model": model(app),
        })),
        "model" => ok(model(app)),
        "ui.widgets" => {
            let filter = req.params.get("filter").and_then(Value::as_str);
            let list: Vec<Value> = app
                .widgets
                .iter()
                .filter(|w| filter.is_none_or(|f| w.id.contains(f)))
                .map(|w| {
                    json!({
                        "id": w.id,
                        "kind": w.kind,
                        "rect": [w.rect.min.x, w.rect.min.y, w.rect.width(), w.rect.height()],
                    })
                })
                .collect();
            ok(json!({"widgets": list, "count": list.len()}))
        }
        "ui.click" => match click(app, &req.params) {
            Ok(rect) => ok(json!({
                "rect": [rect.min.x, rect.min.y, rect.width(), rect.height()]
            })),
            Err(msg) => err(&msg),
        },
        "ui.key" => match key(&req.params) {
            Some(batch) => {
                app.queue_events(batch);
                ok(json!({}))
            }
            None => err("ui.key needs {key: <name>, modifiers?: [ctrl|cmd|shift|alt]}"),
        },
        "ui.type" => match req.params.get("text").and_then(Value::as_str) {
            Some(text) => {
                app.queue_events(vec![Event::Text(text.to_string())]);
                ok(json!({}))
            }
            None => err("ui.type needs {text: <string>}"),
        },
        "ui.scroll" => {
            let amount = req
                .params
                .get("amount")
                .and_then(Value::as_f64)
                .unwrap_or(0.0) as f32;
            app.queue_events(vec![Event::MouseWheel {
                unit: MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -amount),
                phase: TouchPhase::Move,
                modifiers: Modifiers::NONE,
            }]);
            ok(json!({}))
        }
        "ui.screenshot" => {
            let Some(path) = req.params.get("path").and_then(Value::as_str) else {
                return with_id(req.id.clone(), err("ui.screenshot needs {path: <file>}"));
            };
            match ctx {
                None => err("ui.screenshot needs a drawn context"),
                Some(ctx) => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
                        path.to_string(),
                    )));
                    ok(json!({"path": path, "requested": true}))
                }
            }
        }
        "ui.quit" => {
            app.quit = true;
            if let Some(ctx) = ctx {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ok(json!({}))
        }
        "engine.execute" => {
            let Some(command) = req.params.get("command").and_then(Value::as_str) else {
                return with_id(
                    req.id.clone(),
                    err("engine.execute needs {command: <id>} (see spectrum_core::command::describe)"),
                );
            };
            let params = req
                .params
                .get("params")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let ran = app.run(command, params);
            ok(json!({
                "command": command,
                "ran": ran,
                "notice": app.notice.clone(),
                "model": model(app),
            }))
        }
        other => err(&format!("unknown method {other}")),
    };
    with_id(req.id.clone(), result)
}

/// `egui::Key::from_name` wants `ArrowDown`, `A`, `F1`: capitalise the first letter as a fallback.
fn capitalized(name: &str) -> String {
    let mut out = name.chars();
    match out.next() {
        Some(first) => first.to_uppercase().collect::<String>() + out.as_str(),
        None => String::new(),
    }
}

/// Echo the request id, so a client can match replies to requests.
fn with_id(id: Value, mut value: Value) -> Value {
    if let Some(map) = value.as_object_mut() {
        map.insert("id".to_string(), id);
    }
    value
}

fn ok(extra: Value) -> Value {
    let mut out = serde_json::Map::new();
    out.insert("ok".to_string(), json!(true));
    if let Some(map) = extra.as_object() {
        for (key, value) in map {
            if key != "ok" {
                out.insert(key.clone(), value.clone());
            }
        }
    }
    Value::Object(out)
}

fn err(msg: &str) -> Value {
    json!({"ok": false, "error": msg})
}

/// Queue a press and a release on a registered widget's centre: one batch per frame, so egui sees
/// a real press-then-release rather than a single-frame teleport.
fn click(app: &mut AppState, params: &Value) -> Result<Rect, String> {
    let Some(id) = params.get("id").and_then(Value::as_str) else {
        return Err("ui.click needs {id: <widget id>} (see ui.widgets)".to_string());
    };
    let found = app
        .widgets
        .iter()
        .find(|w| w.id == id)
        .or_else(|| app.widgets.iter().find(|w| w.id.contains(id)));
    // Copy the geometry out before queueing: `queue_events` needs `app` mutably.
    let Some(rect) = found.map(|w| w.rect) else {
        return Err(format!(
            "no widget {id} on screen (see ui.widgets; {} widgets registered)",
            app.widgets.len()
        ));
    };
    let center = rect.center();
    app.queue_events(vec![
        Event::PointerMoved(center),
        Event::PointerButton {
            pos: center,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        },
    ]);
    app.queue_events(vec![Event::PointerButton {
        pos: center,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    }]);
    Ok(rect)
}

fn key(params: &Value) -> Option<Vec<Event>> {
    let name = params.get("key").and_then(Value::as_str)?;
    let lower = name.to_ascii_lowercase();
    let key = match lower.as_str() {
        "enter" | "return" => Key::Enter,
        "escape" | "esc" => Key::Escape,
        "tab" => Key::Tab,
        "space" => Key::Space,
        "backspace" => Key::Backspace,
        "delete" => Key::Delete,
        _ if name.len() == 1 => Key::from_name(&name.to_uppercase())?,
        _ => Key::from_name(name).or_else(|| Key::from_name(&capitalized(&lower)))?,
    };
    let mut modifiers = Modifiers::NONE;
    if let Some(list) = params.get("modifiers").and_then(Value::as_array) {
        for item in list {
            match item.as_str() {
                Some("ctrl") | Some("control") => modifiers.ctrl = true,
                Some("cmd") | Some("command") | Some("meta") => modifiers.command = true,
                Some("shift") => modifiers.shift = true,
                Some("alt") => modifiers.alt = true,
                _ => {}
            }
        }
    }
    Some(vec![
        Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        },
        Event::Key {
            key,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers,
        },
    ])
}

/// What the screen shows, in a shape a script can assert on.
fn model(app: &AppState) -> Value {
    let selected: Vec<String> = (0..app.lib.entries.len())
        .filter(|&i| app.lib.selection.contains(i))
        .map(|i| app.lib.entries[i].name())
        .collect();
    json!({
        "folder": app.lib.folder().display().to_string(),
        "folders": app.lib.folders.iter().map(|s| s.label()).collect::<Vec<String>>(),
        "active": app.lib.active,
        "entries": app.lib.entries.len(),
        "visible": app.lib.visible().len(),
        "selected": selected,
        "view": app.lib.view.arg(),
        "tab": app.lib.tab.arg(),
        "filter": {
            "kind": app.lib.filter.kind.map(|k| k.arg()),
            "hdr": app.lib.filter.hdr_only,
        },
        "sort": {"key": app.lib.sort.arg(), "direction": app.lib.direction.label()},
        "notice": app.notice.clone(),
    })
}

/// Parse one request line. `None` on a malformed line, so a bad line cannot kill a connection.
pub fn parse(line: &str) -> Option<(Value, String, Value)> {
    let value: Value = serde_json::from_str(line).ok()?;
    let method = value.get("method")?.as_str()?.to_string();
    let params = value.get("params").cloned().unwrap_or_else(|| json!({}));
    let id = value.get("id").cloned().unwrap_or(Value::Null);
    Some((id, method, params))
}

/// A reply as one line of JSON, ready to write to the socket.
pub fn render(value: &Value) -> String {
    let mut line = value.to_string();
    line.push('\n');
    line
}
