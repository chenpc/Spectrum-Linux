#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

//! Every user action is a command with an id. The UI, the CLI, tests and (later) MCP all dispatch
//! through [`dispatch`] by id, so nothing that a person can do is unreachable from automation.
//!
//! Modelled on LightCraft's `CommandSpec`: `id`, `label`, `group`, `shortcut`, `params`,
//! `enabled` (the menu/shortcut judge) and `run`. A command that cannot apply returns
//! [`CommandError`] — never a panic, never a silent no-op.

use serde_json::{json, Value};

use crate::crop::CropRect;
use crate::edit::EditOp;
use crate::filter::Filter;
use crate::model::{InspectorTab, Library, ViewMode, DETAIL_MAX_SCALE, DETAIL_MIN_SCALE};
use crate::nav::{next_index, Direction};
use crate::select::ClickMods;
use crate::sort::{SortDirection, SortKey};
use crate::trash::DeleteOutcome;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    /// No command with this id.
    Unknown(String),
    /// The command exists but its guard fails; the message says what is missing.
    Disabled(String),
    /// A parameter is absent, the wrong type, or out of range.
    BadParam(String),
    /// The action itself failed (I/O, a denied write).
    Failed(String),
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommandError::Unknown(id) => write!(f, "unknown command `{id}`"),
            CommandError::Disabled(why) => write!(f, "not available: {why}"),
            CommandError::BadParam(why) => write!(f, "bad parameter: {why}"),
            CommandError::Failed(why) => write!(f, "failed: {why}"),
        }
    }
}

pub type Guard = fn(&Library) -> bool;
pub type Handler = fn(&mut Library, &Value) -> Result<Value, CommandError>;

pub struct CommandSpec {
    pub id: &'static str,
    pub label: &'static str,
    /// Menu group; `""` = not in menus.
    pub group: &'static str,
    pub shortcut: Option<&'static str>,
    /// Human- and agent-readable parameter description.
    pub params: &'static str,
    pub enabled: Guard,
    pub run: Handler,
}

const ALL_ENABLED: Guard = |_| true;
const NEEDS_A_FOLDER: Guard = |lib| !lib.folders.is_empty();
const NEEDS_ONE: Guard = |lib| lib.sole().is_some();
const NEEDS_ANY: Guard = |lib| !lib.selection.set.is_empty();

/// Every command the app has. Order is display order (menus list groups in this order).
pub fn specs() -> &'static [CommandSpec] {
    &[
        CommandSpec {
            id: "library.rescan",
            label: "Rescan Folder",
            group: "Library",
            shortcut: Some("Cmd+R"),
            params: "{}",
            enabled: ALL_ENABLED,
            run: |lib, _| {
                lib.rescan();
                Ok(json!({"entries": lib.entries.len()}))
            },
        },
        CommandSpec {
            id: "folder.add",
            label: "Add Folder",
            group: "Library",
            shortcut: Some("Cmd+T"),
            params: "{\"path\": \"/absolute/path\"}",
            enabled: ALL_ENABLED,
            run: |lib, args| {
                let path = str_arg(args, "path")?;
                let index = lib.add_folder(std::path::Path::new(path))?;
                Ok(json!({"index": index, "folder": lib.folder().display().to_string(), "entries": lib.entries.len()}))
            },
        },
        CommandSpec {
            id: "folder.open",
            label: "Open Folder",
            group: "Library",
            shortcut: None,
            params: "{\"index\": 1}",
            enabled: ALL_ENABLED,
            run: |lib, args| {
                let index = usize_arg(args, "index")?;
                lib.open_folder(index)?;
                Ok(json!({"folder": lib.folder().display().to_string(), "entries": lib.entries.len()}))
            },
        },
        CommandSpec {
            id: "folder.remove",
            label: "Remove Folder From Library",
            group: "Library",
            shortcut: None,
            params: "{\"index\": 0}",
            enabled: NEEDS_A_FOLDER,
            run: |lib, args| {
                let index = usize_arg(args, "index")?;
                lib.remove_folder(index)?;
                Ok(json!({"folders": lib.folders.len(), "entries": lib.entries.len()}))
            },
        },
        CommandSpec {
            id: "library.select",
            label: "Select",
            group: "Library",
            shortcut: None,
            params: "{\"indices\": [0, 2]}",
            enabled: ALL_ENABLED,
            run: |lib, args| {
                let indices = usizes(args, "indices")?;
                lib.select(&indices);
                Ok(json!({"count": lib.selection.count()}))
            },
        },
        CommandSpec {
            id: "grid.click",
            label: "Click Item",
            group: "Library",
            shortcut: None,
            params: "{\"index\": 3, \"ctrl\": false, \"shift\": false}",
            enabled: ALL_ENABLED,
            run: |lib, args| {
                let index = usize_arg(args, "index")?;
                if index >= lib.entries.len() {
                    return Err(CommandError::BadParam(format!("index {index} is past the {} items", lib.entries.len())));
                }
                lib.click(index, ClickMods { ctrl: bool_arg(args, "ctrl").unwrap_or(false), shift: bool_arg(args, "shift").unwrap_or(false) });
                Ok(json!({"selected": lib.selection.count(), "primary": lib.selection.primary}))
            },
        },
        CommandSpec {
            id: "library.clear",
            label: "Clear Selection",
            group: "Library",
            shortcut: Some("Cmd+Shift+A"),
            params: "{}",
            enabled: NEEDS_ANY,
            run: |lib, _| {
                lib.clear_selection();
                Ok(json!({"selected": 0}))
            },
        },
        CommandSpec {
            id: "view.set",
            label: "View",
            group: "View",
            shortcut: None,
            params: "{\"view\": \"all\" | \"months\" | \"years\"}",
            enabled: ALL_ENABLED,
            run: |lib, args| {
                let name = str_arg(args, "view")?;
                lib.view = ViewMode::parse(name).ok_or_else(|| CommandError::BadParam(format!("unknown view `{name}`")))?;
                Ok(json!({"view": lib.view.label()}))
            },
        },
        CommandSpec {
            id: "month.open",
            label: "Open Month",
            group: "View",
            shortcut: None,
            params: "{\"key\": \"2016-08\"} or {\"key\": null} for all months",
            enabled: ALL_ENABLED,
            run: |lib, args| {
                match optional_str(args, "key") {
                    Some(key) => {
                        if key.len() != 7 || key.chars().nth(4) != Some('-') {
                            return Err(CommandError::BadParam(format!("month key `{key}` is not YYYY-MM")));
                        }
                        lib.filter.month = Some(key.to_string());
                    }
                    None => lib.filter.month = None,
                }
                lib.view = ViewMode::All;
                Ok(json!({"month": lib.filter.month}))
            },
        },
        CommandSpec {
            id: "year.open",
            label: "Open Year",
            group: "View",
            shortcut: None,
            params: "{\"key\": \"2016\"} or {\"key\": null} for all years",
            enabled: ALL_ENABLED,
            run: |lib, args| {
                match optional_str(args, "key") {
                    Some(key) => {
                        if key.len() != 4 || !key.chars().all(|c| c.is_ascii_digit()) {
                            return Err(CommandError::BadParam(format!("year key `{key}` is not YYYY")));
                        }
                        lib.year = Some(key.to_string());
                        lib.filter.month = None;
                    }
                    None => lib.year = None,
                }
                lib.view = ViewMode::Months;
                Ok(json!({"year": lib.year}))
            },
        },
        CommandSpec {
            id: "group.toggle",
            label: "Collapse / Expand Group",
            group: "View",
            shortcut: None,
            params: "{\"key\": \"2016-08\"}",
            enabled: ALL_ENABLED,
            run: |lib, args| {
                let key = str_arg(args, "key")?;
                lib.toggle_group(key);
                Ok(json!({"collapsed": lib.collapsed.iter().any(|k| k == key)}))
            },
        },
        CommandSpec {
            id: "group.collapseAll",
            label: "Collapse All",
            group: "View",
            shortcut: None,
            params: "{}",
            enabled: ALL_ENABLED,
            run: |lib, _| {
                lib.collapse_all();
                Ok(json!({"groups": lib.collapsed.len()}))
            },
        },
        CommandSpec {
            id: "group.expandAll",
            label: "Expand All",
            group: "View",
            shortcut: None,
            params: "{}",
            enabled: ALL_ENABLED,
            run: |lib, _| {
                lib.expand_all();
                Ok(json!({"groups": 0}))
            },
        },
        CommandSpec {
            id: "select.move",
            label: "Move selection",
            group: "Selection",
            shortcut: Some("Arrows / Home / End"),
            params: "{\"direction\": \"right\", \"columns\": 7}",
            enabled: ALL_ENABLED,
            run: |lib, args| {
                let name = str_arg(args, "direction")?;
                let direction = Direction::parse(name)
                    .ok_or_else(|| CommandError::BadParam(format!("unknown direction {name:?}")))?;
                let columns = usize_arg(args, "columns")?.max(1);
                let positions = lib.visible();
                if positions.is_empty() {
                    return Err(CommandError::Disabled("nothing is visible to select".into()));
                }
                // From the anchor if there is one, else from the last selected row. Neither key is
                // in the visible list when a filter hid it, so a cold start begins at the first.
                let current = lib
                    .selection
                    .primary
                    .or_else(|| lib.selection.set.last().copied())
                    .and_then(|index| positions.iter().position(|&pos| pos == index));
                // Nothing selected yet: the first keypress picks an end rather than doing nothing,
                // so `End` lands on the last item and every other key on the first.
                let next = match current {
                    Some(pos) => next_index(pos, positions.len(), columns, direction),
                    None => Some(if direction == Direction::End {
                        positions.len() - 1
                    } else {
                        0
                    }),
                };
                match next {
                    Some(pos) => {
                        let index = positions[pos];
                        lib.select(std::slice::from_ref(&index));
                        Ok(json!({
                            "moved": true,
                            "index": index,
                            "position": pos,
                            "total": positions.len(),
                        }))
                    }
                    // Past the edge the selection stays put; that is a normal answer, not an error.
                    None => Ok(json!({
                        "moved": false,
                        "position": current.unwrap_or(0),
                        "total": positions.len(),
                    })),
                }
            },
        },
        CommandSpec {
            id: "select.clear",
            label: "Clear selection",
            group: "Selection",
            shortcut: Some("Esc"),
            params: "{}",
            enabled: ALL_ENABLED,
            run: |lib, _| {
                let had = lib.selection.count();
                lib.clear_selection();
                Ok(json!({"cleared": had}))
            },
        },
        CommandSpec {
            id: "detail.open",
            label: "Open Detail",
            group: "View",
            shortcut: Some("Return"),
            params: "{}",
            enabled: ALL_ENABLED,
            run: |lib, _| {
                let index = lib.sole().ok_or_else(|| {
                    CommandError::Disabled("the detail view needs exactly one selected item".into())
                })?;
                lib.detail = Some(index);
                lib.detail_zoom = None;
                Ok(json!({"index": index, "file": lib.entries[index].name()}))
            },
        },
        CommandSpec {
            id: "detail.close",
            label: "Close Detail",
            group: "View",
            shortcut: Some("Esc"),
            params: "{}",
            enabled: ALL_ENABLED,
            run: |lib, _| {
                let had = lib.detail.take().is_some();
                lib.detail_zoom = None;
                Ok(json!({"closed": had}))
            },
        },
        CommandSpec {
            id: "detail.zoom",
            label: "Zoom",
            group: "View",
            shortcut: Some("Ctrl+= / Ctrl+-"),
            params: "{\"action\": \"in\" | \"out\" | \"fit\" | \"one\", \"current\": 0.42}",
            enabled: ALL_ENABLED,
            run: |lib, args| {
                if lib.detail.is_none() {
                    return Err(CommandError::Disabled("nothing is open in the detail view".into()));
                }
                let action = str_arg(args, "action")?;
                // `current` is the scale the canvas is showing. From `fit` the registry cannot know
                // the canvas size, so the UI passes what it drew; without it the base is 1:1.
                let base = zoom_base(args, lib);
                let zoom = match action {
                    "in" => base * 1.25,
                    "out" => base * 0.8,
                    "one" => 1.0,
                    "fit" => {
                        lib.detail_zoom = None;
                        return Ok(json!({"zoom": null, "fit": true}));
                    }
                    other => {
                        return Err(CommandError::BadParam(format!(
                            "unknown zoom action {other:?}"
                        )))
                    }
                };
                lib.detail_zoom = Some(zoom.clamp(DETAIL_MIN_SCALE, DETAIL_MAX_SCALE));
                Ok(json!({"zoom": lib.detail_zoom}))
            },
        },
        CommandSpec {
            id: "hdr.toggle",
            label: "HDR/SDR toggle",
            group: "View",
            shortcut: Some("h"),
            params: "{}",
            enabled: ALL_ENABLED,
            run: |lib, _| {
                lib.hdr_enabled = !lib.hdr_enabled;
                Ok(json!({"hdr": lib.hdr_enabled}))
            },
        },
        CommandSpec {
            id: "inspector.tab",
            label: "Inspector Tab",
            group: "View",
            shortcut: Some("i"),
            params: "{\"tab\": \"adjust\" | \"info\"}",
            enabled: ALL_ENABLED,
            run: |lib, args| {
                let name = str_arg(args, "tab")?;
                lib.tab = InspectorTab::parse(name).ok_or_else(|| CommandError::BadParam(format!("unknown tab `{name}`")))?;
                Ok(json!({"tab": lib.tab.label()}))
            },
        },
        CommandSpec {
            id: "filter.kind",
            label: "Kind Filter",
            group: "Filter",
            shortcut: None,
            params: "{\"kind\": \"photo\" | \"video\" | null}",
            enabled: ALL_ENABLED,
            run: |lib, args| {
                match optional_str(args, "kind") {
                    Some(name) => {
                        lib.filter.kind = Some(crate::scan::MediaKind::parse(name).ok_or_else(|| {
                            CommandError::BadParam(format!("unknown kind `{name}`"))
                        })?);
                    }
                    None => lib.filter.kind = None,
                }
                Ok(json!({"kind": lib.filter.kind.map(|k| k.label())}))
            },
        },
        CommandSpec {
            id: "filter.hdr",
            label: "HDR Only",
            group: "Filter",
            shortcut: None,
            params: "{\"on\": true}",
            enabled: ALL_ENABLED,
            run: |lib, args| {
                lib.filter.hdr_only = bool_arg(args, "on").ok_or_else(|| CommandError::BadParam("missing `on`".to_string()))?;
                Ok(json!({"visible": lib.order().len()}))
            },
        },
        CommandSpec {
            id: "filter.clear",
            label: "Clear Filters",
            group: "Filter",
            shortcut: None,
            params: "{}",
            enabled: ALL_ENABLED,
            run: |lib, _| {
                lib.filter = Filter::default();
                lib.year = None;
                Ok(json!({"visible": lib.order().len()}))
            },
        },
        CommandSpec {
            id: "sort.set",
            label: "Sort By",
            group: "Filter",
            shortcut: None,
            params: "{\"key\": \"name\" | \"date\" | \"kind\" | \"transfer\" | \"size\", \"direction\": \"natural\" | \"reversed\"}",
            enabled: ALL_ENABLED,
            run: |lib, args| {
                let key = str_arg(args, "key")?;
                lib.sort = SortKey::parse(key).ok_or_else(|| CommandError::BadParam(format!("unknown sort key `{key}`")))?;
                match optional_str(args, "direction") {
                    Some(name) => {
                        lib.direction = SortDirection::parse(name)
                            .ok_or_else(|| CommandError::BadParam(format!("unknown direction `{name}`")))?;
                    }
                    None => lib.direction = SortDirection::Natural,
                }
                Ok(json!({"key": lib.sort.arg(), "direction": lib.direction.label()}))
            },
        },
        CommandSpec {
            id: "edit.rotate",
            label: "Rotate",
            group: "Edit",
            shortcut: Some("r"),
            params: "{\"dir\": 90 | -90}",
            enabled: NEEDS_ONE,
            run: |lib, args| {
                let dir = i64_arg(args, "dir").unwrap_or(-90);
                if dir != 90 && dir != -90 {
                    return Err(CommandError::BadParam(format!("`dir` must be 90 or -90, got {dir}")));
                }
                let index = sole(lib)?;
                let edit = lib.apply_edit(index, EditOp::Rotate(dir as i32))?;
                Ok(edit_json(&edit))
            },
        },
        CommandSpec {
            id: "edit.flipH",
            label: "Flip Horizontal",
            group: "Edit",
            shortcut: Some("Cmd+Shift+H"),
            params: "{}",
            enabled: NEEDS_ONE,
            run: |lib, _| {
                let index = sole(lib)?;
                let edit = lib.apply_edit(index, EditOp::FlipH)?;
                Ok(edit_json(&edit))
            },
        },
        CommandSpec {
            id: "edit.crop",
            label: "Crop",
            group: "Edit",
            shortcut: None,
            params: "{\"x\": 0.1, \"y\": 0.2, \"width\": 0.6, \"height\": 0.5}",
            enabled: NEEDS_ONE,
            run: |lib, args| {
                let rect = CropRect::new(f64_arg(args, "x")?, f64_arg(args, "y")?, f64_arg(args, "width")?, f64_arg(args, "height")?);
                if rect.width <= 0.0 || rect.height <= 0.0 || rect.x < 0.0 || rect.y < 0.0 || rect.x + rect.width > 1.0 || rect.y + rect.height > 1.0 {
                    return Err(CommandError::BadParam(format!("crop {rect:?} is not inside 0..1")));
                }
                let index = sole(lib)?;
                let edit = lib.apply_edit(index, EditOp::Crop(rect))?;
                Ok(edit_json(&edit))
            },
        },
        CommandSpec {
            id: "edit.restore",
            label: "Restore Original",
            group: "Edit",
            shortcut: Some("Cmd+Z"),
            params: "{}",
            enabled: NEEDS_ONE,
            run: |lib, _| {
                let index = sole(lib)?;
                lib.restore(index)?;
                Ok(json!({"restored": lib.entries.get(index).map(|e| e.name())}))
            },
        },
        CommandSpec {
            id: "photo.trash",
            label: "Move to Trash",
            group: "Photo",
            shortcut: Some("Delete"),
            params: "{}",
            enabled: NEEDS_ANY,
            run: |lib, _| {
                let outcomes = lib.trash_selected(std::time::SystemTime::now())?;
                let moved = outcomes.iter().filter(|(_, o)| matches!(o, DeleteOutcome::Trashed(_))).count();
                Ok(json!({"moved": moved, "total": outcomes.len(), "status": lib.status.text()}))
            },
        },
        CommandSpec {
            id: "status.clear",
            label: "Clear Status Bar",
            group: "Photo",
            shortcut: None,
            params: "{}",
            enabled: ALL_ENABLED,
            run: |lib, _| {
                lib.status.clear();
                Ok(json!({"visible": lib.status.is_visible()}))
            },
        },
    ]
}

pub fn lookup(id: &str) -> Option<&'static CommandSpec> {
    specs().iter().find(|s| s.id == id)
}

/// One-line description of every command, for `spectrum-cli commands --json`.
pub fn describe() -> Value {
    json!(specs()
        .iter()
        .map(|s| json!({"id": s.id, "label": s.label, "group": s.group, "shortcut": s.shortcut, "params": s.params}))
        .collect::<Vec<Value>>())
}

/// Run a command by id. The guard is checked first, so a disabled command never runs.
pub fn dispatch(model: &mut Library, id: &str, args: &Value) -> Result<Value, CommandError> {
    let spec = lookup(id).ok_or_else(|| CommandError::Unknown(id.to_string()))?;
    if !(spec.enabled)(model) {
        return Err(CommandError::Disabled(disabled_reason(model, id)));
    }
    (spec.run)(model, args)
}

fn disabled_reason(model: &Library, id: &str) -> String {
    if id == "folder.remove" {
        return "the library has no folders".to_string();
    }
    if id.starts_with("edit.") || id == "edit.restore" {
        return match model.selection.set.len() {
            0 => "no photo is selected".to_string(),
            n => format!("select exactly one photo, {n} are selected"),
        };
    }
    "nothing is selected".to_string()
}

fn sole(lib: &Library) -> Result<usize, CommandError> {
    lib.sole()
        .ok_or_else(|| CommandError::Disabled("select exactly one photo".to_string()))
}

fn edit_json(edit: &crate::edit::CompositeEdit) -> Value {
    json!({
        "rotation": edit.rotation,
        "flipH": edit.flip_h,
        "crop": edit.crop.map(|c| json!({"x": c.x, "y": c.y, "width": c.width, "height": c.height})),
    })
}

// --- argument readers: every one returns an error, none panics ---

fn want_object(args: &Value) -> Result<&Value, CommandError> {
    if args.is_object() || args.is_null() {
        Ok(args)
    } else {
        Err(CommandError::BadParam(format!(
            "parameters must be an object, got {}",
            json_kind(args)
        )))
    }
}

fn json_kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn field<'a>(args: &'a Value, key: &str) -> Option<&'a Value> {
    args.get(key).filter(|v| !v.is_null())
}

/// The scale the detail canvas is showing: an explicit zoom, else the `current` the UI passed
/// (the fit scale needs the canvas size, which the registry does not know), else 1:1.
fn zoom_base(args: &Value, lib: &Library) -> f32 {
    lib.detail_zoom.unwrap_or_else(|| {
        field(args, "current")
            .and_then(Value::as_f64)
            .map(|v| v as f32)
            .unwrap_or(1.0)
    })
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Result<&'a str, CommandError> {
    want_object(args)?;
    field(args, key)
        .and_then(Value::as_str)
        .ok_or_else(|| CommandError::BadParam(format!("missing string `{key}`")))
}

fn optional_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    field(args, key).and_then(Value::as_str)
}

fn bool_arg(args: &Value, key: &str) -> Option<bool> {
    field(args, key).and_then(Value::as_bool)
}

fn i64_arg(args: &Value, key: &str) -> Option<i64> {
    field(args, key).and_then(Value::as_i64)
}

fn usize_arg(args: &Value, key: &str) -> Result<usize, CommandError> {
    want_object(args)?;
    i64_arg(args, key)
        .and_then(|v| usize::try_from(v).ok())
        .ok_or_else(|| CommandError::BadParam(format!("missing non-negative integer `{key}`")))
}

fn f64_arg(args: &Value, key: &str) -> Result<f64, CommandError> {
    want_object(args)?;
    field(args, key)
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite())
        .ok_or_else(|| CommandError::BadParam(format!("missing finite number `{key}`")))
}

fn usizes(args: &Value, key: &str) -> Result<Vec<usize>, CommandError> {
    want_object(args)?;
    let list = field(args, key)
        .and_then(Value::as_array)
        .ok_or_else(|| CommandError::BadParam(format!("missing array `{key}`")))?;
    let mut out = Vec::with_capacity(list.len());
    for item in list {
        out.push(
            item.as_i64()
                .and_then(|v| usize::try_from(v).ok())
                .ok_or_else(|| {
                    CommandError::BadParam(format!("`{key}` holds a non-integer: {item}"))
                })?,
        );
    }
    Ok(out)
}
