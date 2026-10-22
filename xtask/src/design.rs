#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

//! `cargo xtask design`: read the FigCraft document and emit the measured design tokens.
//!
//! - **Observed** (`assets/design/observed.json`, generated): every screen, palette colour, type
//!   size, corner radius, layout gap/padding, component box and prototype flow that actually
//!   exists in the `.figcraft` file, with counts. Regenerate with `design --write`; `ci` fails
//!   when the committed file no longer matches the design source.
//! - **Theme** (`assets/design/theme.json`, authored): the semantic roles the app uses. Checked
//!   against the observed palette, so a role that drifts away from the design is caught in CI.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// Object entries, or none, without borrowing a temporary.
fn object_pairs(map: Option<&serde_json::Map<String, Value>>) -> Vec<(&String, &Value)> {
    map.map(|m| m.iter().collect()).unwrap_or_default()
}

fn array_items(value: Option<&Value>) -> &[Value] {
    value
        .and_then(Value::as_array)
        .map(|a| a.as_slice())
        .unwrap_or(&[])
}

/// Component boxes worth recording by name; anything else is captured by the histograms.
const INTERESTING: &[&str] = &[
    "Titlebar",
    "Titlebar divider",
    "Sidebar",
    "Sidebar divider",
    "Main",
    "Viewer",
    "Photo",
    "Caption",
    "Inspector",
    "Inspector tabs",
    "Library popup",
    "View switch",
    "Search",
    "Import",
    "Share",
    "Edit",
    "Info",
    "Duplicate",
    "Add to Album",
    "Zoom",
    "Month header",
    "Photo grid",
    "Month tile",
    "Memory card",
    "Filter chips",
    "Exposure",
    "Brightness",
    "Albums header",
    "Photos",
    "Memories",
    "Recently Deleted",
];

/// The `.figcraft` file is a zip holding `document.json`.
pub fn read_fig(path: &Path) -> Result<Value, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("{} is not a zip archive: {e}", path.display()))?;
    let mut entry = archive
        .by_name("document.json")
        .map_err(|e| format!("{} holds no document.json: {e}", path.display()))?;
    let mut text = String::new();
    entry
        .read_to_string(&mut text)
        .map_err(|e| format!("read document.json from {}: {e}", path.display()))?;
    serde_json::from_str(&text)
        .map_err(|e| format!("parse document.json in {}: {e}", path.display()))
}

pub fn fig_sha256(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(hex_of(&hasher.finalize()))
}

fn hex_of(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

fn channel(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64)
}

/// `#rrggbb`, plus `@NN%` when the alpha is not fully opaque.
fn hex_color(color: Option<&Value>) -> Option<String> {
    let c = color?;
    let r = channel(c.get("r"))?;
    let g = channel(c.get("g"))?;
    let b = channel(c.get("b"))?;
    let a = channel(c.get("a")).unwrap_or(1.0);
    let base = format!(
        "#{:02x}{:02x}{:02x}",
        (r * 255.0).round() as u8,
        (g * 255.0).round() as u8,
        (b * 255.0).round() as u8,
    );
    if a >= 0.995 {
        Some(base)
    } else {
        Some(format!("{base}@{}", (a * 100.0).round() as u32))
    }
}

fn fill_colors(node: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for fill in node
        .get("fills")
        .and_then(Value::as_array)
        .map(|a| a.as_slice())
        .unwrap_or(&[])
    {
        if fill.get("visible").and_then(Value::as_bool) == Some(false) {
            continue;
        }
        match fill.get("type").and_then(Value::as_str) {
            Some("solid") | None => {
                if let Some(hex) = hex_color(fill.get("color")) {
                    out.push(hex);
                }
            }
            Some(other) => out.push(format!("gradient:{other}")),
        }
    }
    out
}

fn first_text_style(nodes: &BTreeMap<String, Value>, id: i64) -> Option<Value> {
    let node = nodes.get(&id.to_string())?;
    if node.get("type").and_then(Value::as_str) == Some("text") {
        let style = node.get("style")?;
        return Some(json!({
            "characters": node.get("characters").and_then(Value::as_str).unwrap_or(""),
            "family": style.get("font_family").and_then(Value::as_str).unwrap_or(""),
            "weight_name": style.get("font_style").and_then(Value::as_str).unwrap_or(""),
            "size": style.get("font_size"),
        }));
    }
    for child in node
        .get("children")
        .and_then(Value::as_array)
        .map(|a| a.as_slice())
        .unwrap_or(&[])
    {
        let child_id = child.as_i64()?;
        if let Some(found) = first_text_style(nodes, child_id) {
            return Some(found);
        }
    }
    None
}

fn screen_of(nodes: &BTreeMap<String, Value>, mut id: i64) -> String {
    let mut guard = 0;
    while guard < 64 {
        guard += 1;
        let Some(node) = nodes.get(&id.to_string()) else {
            return "?".to_string();
        };
        let parent = node.get("parent").and_then(Value::as_i64);
        match parent {
            Some(p) => {
                if nodes
                    .get(&p.to_string())
                    .and_then(|n| n.get("type"))
                    .and_then(Value::as_str)
                    == Some("page")
                {
                    return node
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("?")
                        .to_string();
                }
                id = p;
            }
            None => return "?".to_string(),
        }
    }
    "?".to_string()
}

fn bump<T: Ord + Clone>(hist: &mut BTreeMap<T, usize>, key: T) {
    *hist.entry(key).or_insert(0) += 1;
}

fn sorted_by_count<T: Ord + Clone + serde::Serialize>(hist: &BTreeMap<T, usize>) -> Vec<Value> {
    let mut items: Vec<(T, usize)> = hist.iter().map(|(k, v)| (k.clone(), *v)).collect();
    items.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    items
        .into_iter()
        .map(|(value, count)| json!({ "value": value, "count": count }))
        .collect()
}

/// Everything measurable in the design document.
pub fn observe(doc: &Value, fig: &Path, sha: &str) -> Result<Value, String> {
    let nodes_map = doc
        .get("nodes")
        .and_then(Value::as_object)
        .ok_or("document.json has no `nodes` object")?;
    let mut nodes: BTreeMap<String, Value> = BTreeMap::new();
    for (key, value) in nodes_map {
        nodes.insert(key.clone(), value.clone());
    }

    let mut palette: BTreeMap<String, usize> = BTreeMap::new();
    let mut type_scale: BTreeMap<(String, String, i64), usize> = BTreeMap::new();
    let mut radii: BTreeMap<String, usize> = BTreeMap::new();
    let mut gaps: BTreeMap<i64, usize> = BTreeMap::new();
    let mut paddings: BTreeMap<String, usize> = BTreeMap::new();
    let mut modes: BTreeMap<String, usize> = BTreeMap::new();
    let mut sizing: BTreeMap<String, usize> = BTreeMap::new();
    let mut components: Vec<Value> = Vec::new();
    let mut screens: Vec<Value> = Vec::new();
    let mut flows: Vec<Value> = Vec::new();

    for (key, node) in &nodes {
        let Ok(id) = key.parse::<i64>() else { continue };
        let kind = node.get("type").and_then(Value::as_str).unwrap_or("");
        let name = node.get("name").and_then(Value::as_str).unwrap_or("");

        for color in fill_colors(node) {
            bump(&mut palette, color);
        }
        if kind == "text" {
            if let Some(style) = node.get("style") {
                let family = style
                    .get("font_family")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let weight = style
                    .get("font_style")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let size = style
                    .get("font_size")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0);
                bump(
                    &mut type_scale,
                    (family, weight, (size * 10.0).round() as i64),
                );
            }
        }
        if let Some(r) = node
            .get("corners")
            .and_then(|c| c.get("radii"))
            .and_then(Value::as_array)
        {
            let parts: Vec<String> = r.iter().map(|v| fmt_num(v.as_f64())).collect();
            bump(&mut radii, parts.join(","));
        }
        if let Some(layout) = node.get("layout") {
            bump(
                &mut modes,
                layout
                    .get("mode")
                    .and_then(Value::as_str)
                    .unwrap_or("none")
                    .to_string(),
            );
            bump(
                &mut gaps,
                (layout.get("gap").and_then(Value::as_f64).unwrap_or(0.0) * 10.0).round() as i64,
            );
            let pad: Vec<String> = layout
                .get("padding")
                .and_then(Value::as_array)
                .map(|a| a.iter().map(|v| fmt_num(v.as_f64())).collect())
                .unwrap_or_default();
            bump(&mut paddings, pad.join(","));
        }
        if let Some(child_layout) = node.get("child_layout") {
            let h = child_layout
                .get("sizing_h")
                .and_then(Value::as_str)
                .unwrap_or("");
            let v = child_layout
                .get("sizing_v")
                .and_then(Value::as_str)
                .unwrap_or("");
            bump(&mut sizing, format!("{h}/{v}"));
        }
        if INTERESTING.contains(&name) {
            components.push(json!({
                "screen": screen_of(&nodes, id),
                "name": name,
                "type": kind,
                "width": node.get("width").and_then(Value::as_f64),
                "height": node.get("height").and_then(Value::as_f64),
                "fills": fill_colors(node),
                "radius": node.get("corners").and_then(|c| c.get("radii")).and_then(Value::as_array).and_then(|a| a.first()).and_then(Value::as_f64),
                "layout": node.get("layout").map(|l| json!({
                    "mode": l.get("mode"),
                    "gap": l.get("gap"),
                    "padding": l.get("padding"),
                    "main_align": l.get("main_align"),
                    "cross_align": l.get("cross_align"),
                })),
                "sizing": node.get("child_layout").map(|c| json!({"h": c.get("sizing_h"), "v": c.get("sizing_v")})),
                "text": first_text_style(&nodes, id),
            }));
        }
        for action in node
            .get("interactions")
            .and_then(Value::as_array)
            .map(|a| a.as_slice())
            .unwrap_or(&[])
        {
            let dest = action
                .get("actions")
                .and_then(Value::as_array)
                .and_then(|a| a.first())
                .and_then(|a| a.get("dest"))
                .and_then(Value::as_i64);
            let Some(dest_id) = dest else { continue };
            flows.push(json!({
                "screen": screen_of(&nodes, id),
                "from": name,
                "to": nodes.get(&dest_id.to_string()).and_then(|n| n.get("name")).and_then(Value::as_str).unwrap_or("?"),
                "trigger": action.get("trigger").and_then(|t| t.get("type")).and_then(Value::as_str),
                "transition": action.get("transition").and_then(|t| t.get("type")).and_then(Value::as_str),
                "duration_ms": action.get("duration_ms"),
                "easing": action.get("easing"),
            }));
        }
    }

    let page = nodes
        .values()
        .find(|n| n.get("type").and_then(Value::as_str) == Some("page"));
    if let Some(page) = page {
        for child in page
            .get("children")
            .and_then(Value::as_array)
            .map(|a| a.as_slice())
            .unwrap_or(&[])
        {
            let Some(id) = child.as_i64() else { continue };
            let Some(screen) = nodes.get(&id.to_string()) else {
                continue;
            };
            screens.push(json!({
                "name": screen.get("name").and_then(Value::as_str).unwrap_or("?"),
                "width": screen.get("width").and_then(Value::as_f64),
                "height": screen.get("height").and_then(Value::as_f64),
                "fills": fill_colors(screen),
            }));
        }
    }

    screens.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .cmp(b["name"].as_str().unwrap_or(""))
    });
    components.sort_by(|a, b| {
        a["screen"]
            .as_str()
            .unwrap_or("")
            .cmp(b["screen"].as_str().unwrap_or(""))
            .then(
                a["name"]
                    .as_str()
                    .unwrap_or("")
                    .cmp(b["name"].as_str().unwrap_or("")),
            )
    });

    let types: Vec<Value> = type_scale
        .iter()
        .map(|((family, weight, size_tenths), count)| json!({ "family": family, "weight_name": weight, "size": *size_tenths as f64 / 10.0, "count": count }))
        .collect();

    Ok(json!({
        "source": {
            "path": fig.display().to_string(),
            "sha256": sha,
            "document_name": doc.get("name").and_then(Value::as_str),
            "format": doc.get("format").and_then(Value::as_str),
            "version": doc.get("version"),
            "nodes": nodes.len(),
        },
        "screens": screens,
        "components": components,
        "flows": flows,
        "palette": sorted_by_count(&palette),
        "type_scale": types,
        "corner_radii": sorted_by_count(&radii),
        "layout": {
            "modes": sorted_by_count(&modes),
            "gap_tenths": sorted_by_count(&gaps),
            "paddings": sorted_by_count(&paddings),
            "child_sizing": sorted_by_count(&sizing),
        },
    }))
}

fn fmt_num(v: Option<f64>) -> String {
    match v {
        Some(n) if n == n.trunc() => format!("{}", n as i64),
        Some(n) => format!("{n}"),
        None => "0".to_string(),
    }
}

/// The authored roles, verified against the observed palette.
pub fn check_theme(theme: &Value, observed: &Value) -> Result<(), String> {
    let mut known: Vec<String> = Vec::new();
    for entry in array_items(observed.get("palette")) {
        if let Some(hex) = entry.get("value").and_then(Value::as_str) {
            known.push(hex.to_string());
        }
    }
    let mut problems = Vec::new();
    for (role, value) in object_pairs(theme.get("color").and_then(Value::as_object)) {
        let Some(hex) = value.as_str() else {
            problems.push(format!("color.{role} is not a string"));
            continue;
        };
        let plain = hex.split('@').next().unwrap_or(hex).to_string();
        if !known.contains(&plain) {
            problems.push(format!(
                "color.{role} = {hex} is not in the design's palette"
            ));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n  "))
    }
}
