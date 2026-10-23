//! The folder list survives a restart.
//!
//! The macOS version stores security-scoped bookmarks; on Linux there is no such thing, so the
//! list is a plain JSON file and each path is re-checked at load time. A folder that vanished
//! (unmounted volume, moved backup) is dropped from the list and reported — it never comes back as
//! an empty folder, and nothing outside the list is ever touched.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::model::FolderSlot;

/// Bumped when the file layout changes, so an old file is recognised rather than misread.
pub const LIBRARY_STORE_VERSION: u64 = 1;

/// What a store file contained, with missing directories already filtered out.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Saved {
    pub folders: Vec<FolderSlot>,
    /// Index into `folders`; clamped to the surviving list.
    pub active: usize,
    /// Paths in the file that are no longer directories, in file order.
    pub dropped: Vec<PathBuf>,
    /// A folder entry that named a directory already in the list.
    pub duplicates: usize,
}

/// `<config_dir>/spectrum/library.json`. `config_dir` is what `$XDG_CONFIG_HOME` (or `$HOME/.config`)
/// resolves to; kept a parameter so a test can point it at a temp directory.
pub fn path_in(config_dir: &Path) -> PathBuf {
    config_dir.join("spectrum").join("library.json")
}

/// The store path for this machine: `$SPECTRUM_LIBRARY_STORE` wins (used by tests and by anyone
/// keeping a repo-local store), then `$XDG_CONFIG_HOME`, then `$HOME/.config`. `None` when neither
/// environment variable is set, in which case nothing is read or written.
pub fn default_path() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("SPECTRUM_LIBRARY_STORE") {
        let path = PathBuf::from(explicit);
        if path.is_empty() {
            return None;
        }
        return Some(path);
    }
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(path_in(&config))
}

/// Put a loaded list into a `Library` and scan it, so the sidebar counts are right immediately.
pub fn apply(library: &mut crate::model::Library, saved: &Saved) {
    library.folders = saved.folders.clone();
    library.active = saved.active;
    library.rescan();
}

/// Write the folder list. The directory is created on demand; the parent directory of an explicit
/// `path` is used as-is.
pub fn save(path: &Path, folders: &[FolderSlot], active: usize) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    let entries: Vec<Value> = folders
        .iter()
        .map(|slot| {
            serde_json::json!({
                "path": slot.path.to_string_lossy(),
                "count": slot.count.map(|n| n as u64),
            })
        })
        .collect();
    let document = serde_json::json!({
        "version": LIBRARY_STORE_VERSION,
        "active": active as u64,
        "folders": entries,
    });
    let text = serde_json::to_string_pretty(&document)
        .map_err(|e| format!("cannot encode the folder list: {e}"))?;
    // Write-then-rename: a crash mid-write leaves the previous file readable.
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, text.as_bytes())
        .map_err(|e| format!("cannot write {}: {e}", temp.display()))?;
    std::fs::rename(&temp, path).map_err(|e| format!("cannot replace {}: {e}", path.display()))?;
    Ok(())
}

/// Read the folder list. A missing file is an empty list, not an error; a malformed file is an
/// error the caller reports instead of silently discarding someone's folders.
pub fn load(path: &Path) -> Result<Saved, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Saved::default()),
        Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
    };
    let document: Value = serde_json::from_str(&text)
        .map_err(|e| format!("{} is not valid JSON: {e}", path.display()))?;
    let version = document.get("version").and_then(Value::as_u64);
    if version != Some(LIBRARY_STORE_VERSION) {
        return Err(format!(
            "{} has version {:?}, this build reads {LIBRARY_STORE_VERSION}",
            path.display(),
            version
        ));
    }
    let entries = match document.get("folders") {
        Some(Value::Array(entries)) => entries.clone(),
        Some(_) => return Err(format!("{}: `folders` is not a list", path.display())),
        None => Vec::new(),
    };

    let mut saved = Saved::default();
    for entry in &entries {
        let path_text = entry.get("path").and_then(Value::as_str).unwrap_or("");
        if path_text.is_empty() {
            continue;
        }
        let candidate = PathBuf::from(path_text);
        if !candidate.is_dir() {
            saved.dropped.push(candidate);
            continue;
        }
        if saved.folders.iter().any(|slot| slot.path == candidate) {
            saved.duplicates += 1;
            continue;
        }
        let count = entry
            .get("count")
            .and_then(Value::as_u64)
            .map(|n| n as usize);
        saved.folders.push(FolderSlot {
            path: candidate,
            count,
        });
    }
    let active = document.get("active").and_then(Value::as_u64).unwrap_or(0) as usize;
    saved.active = if saved.folders.is_empty() {
        0
    } else {
        active.min(saved.folders.len() - 1)
    };
    Ok(saved)
}
