//! Clipboard for the file list (Copy / Cut) and paste into a folder, plus folder rename.

use std::fs;
use std::path::{Path, PathBuf};

use crate::trash::unique_name;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ClipboardMode {
    #[default]
    Copy,
    Cut,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Clipboard {
    pub mode: ClipboardMode,
    pub paths: Vec<PathBuf>,
}

pub fn set_clipboard(paths: &[PathBuf], mode: ClipboardMode) -> Clipboard {
    Clipboard {
        mode,
        paths: paths.to_vec(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasteOutcome {
    pub source: PathBuf,
    /// Destination path (collision names get the " 1" counter, same rule as the trash).
    pub dest: PathBuf,
    pub error: Option<String>,
}

/// Copy keeps the source, Cut moves it. The clipboard is left intact so a paste can be repeated.
pub fn paste(clip: &Clipboard, dest_dir: &Path) -> Vec<PasteOutcome> {
    let mut out = Vec::new();
    for src in &clip.paths {
        if !src.exists() {
            out.push(PasteOutcome {
                source: src.clone(),
                dest: PathBuf::new(),
                error: Some("not found".to_string()),
            });
            continue;
        }
        if !dest_dir.is_dir() {
            out.push(PasteOutcome {
                source: src.clone(),
                dest: PathBuf::new(),
                error: Some(format!("{} is not a directory", dest_dir.display())),
            });
            continue;
        }
        let name = src
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let target = dest_dir.join(unique_name(dest_dir, &name));
        let res = match clip.mode {
            ClipboardMode::Cut => fs::rename(src, &target),
            ClipboardMode::Copy => fs::copy(src, &target).map(|_| ()),
        };
        out.push(PasteOutcome {
            source: src.clone(),
            dest: target,
            error: res.err().map(|e| e.to_string()),
        });
    }
    out
}

pub fn rename_dir(path: &Path, new_name: &str) -> Result<PathBuf, String> {
    if new_name.is_empty() {
        return Err("empty name".to_string());
    }
    if new_name.contains('/') || new_name.contains('\0') {
        return Err(format!("invalid name: {new_name}"));
    }
    if !path.is_dir() {
        return Err(format!("{} is not a directory", path.display()));
    }
    let target = path.parent().unwrap_or(path).join(new_name);
    if target.exists() {
        return Err(format!("already exists: {}", target.display()));
    }
    fs::rename(path, &target).map_err(|e| e.to_string())?;
    Ok(target)
}
