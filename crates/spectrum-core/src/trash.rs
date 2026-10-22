//! Delete-to-trash following the freedesktop Trash spec, with a permanent-delete fallback.
//!
//! Layout: `<root>/files/<name>` plus `<root>/info/<name>.trashinfo` containing
//! `[Trash Info]`, `Path=<absolute original path>`, `Time=<ISO 8601 UTC>`.
//! The root is passed in so tests never touch the user's real trash.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::timeline::civil_from_days;

/// `SPECTRUM_TRASH_ROOT` overrides the destination so tests never touch the user's real trash.
pub fn trash_root() -> PathBuf {
    match std::env::var("SPECTRUM_TRASH_ROOT") {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => default_trash_root(),
    }
}

pub fn default_trash_root() -> PathBuf {
    let data = std::env::var("XDG_DATA_HOME").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_default();
        format!("{home}/.local/share")
    });
    PathBuf::from(data).join("Trash")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteOutcome {
    /// Path inside `<root>/files` the file was moved to.
    Trashed(PathBuf),
    /// Trash was unavailable, so the file was deleted permanently.
    Removed,
    Failed(String),
}

/// Name collision rule from the spec: append " 1", " 2", ... before the extension is kept as-is
/// (the whole name, including any extension, gets the suffix).
pub(crate) fn unique_name(files_dir: &Path, name: &str) -> String {
    if !files_dir.join(name).exists() {
        return name.to_string();
    }
    let mut n = 1;
    loop {
        let candidate = format!("{name} {n}");
        if !files_dir.join(&candidate).exists() {
            return candidate;
        }
        n += 1;
    }
}

pub fn iso8601_utc(t: SystemTime) -> String {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => {
            let secs = d.as_secs() as i64;
            let (y, m, day) = civil_from_days(secs / 86400);
            format!(
                "{y:04}-{m:02}-{day:02}T{:02}:{:02}:{:02}",
                (secs % 86400) / 3600,
                (secs % 3600) / 60,
                secs % 60
            )
        }
        Err(e) => {
            let secs = -(e.duration().as_secs() as i64);
            let (y, m, day) = civil_from_days(secs.div_euclid(86400));
            format!(
                "{y:04}-{m:02}-{day:02}T{:02}:{:02}:{:02}",
                secs.rem_euclid(86400) / 3600,
                secs.rem_euclid(3600) / 60,
                secs.rem_euclid(60)
            )
        }
    }
}

pub fn trashinfo_contents(original: &Path, time: &str) -> String {
    format!("[Trash Info]\nPath={}\nTime={}\n", original.display(), time)
}

pub fn delete_paths(paths: &[PathBuf], root: &Path, time: &str) -> Vec<(PathBuf, DeleteOutcome)> {
    let files_dir = root.join("files");
    let info_dir = root.join("info");
    let trash_available = fs::create_dir_all(&files_dir)
        .and_then(|_| fs::create_dir_all(&info_dir))
        .is_ok();

    let mut out = Vec::new();
    for path in paths {
        if !path.exists() {
            out.push((path.clone(), DeleteOutcome::Failed("not found".to_string())));
            continue;
        }
        if !trash_available {
            match fs::remove_file(path) {
                Ok(()) => out.push((path.clone(), DeleteOutcome::Removed)),
                Err(e) => out.push((path.clone(), DeleteOutcome::Failed(e.to_string()))),
            }
            continue;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let stored = unique_name(&files_dir, &name);
        let dest = files_dir.join(&stored);
        if let Err(e) = fs::rename(path, &dest) {
            out.push((path.clone(), DeleteOutcome::Failed(e.to_string())));
            continue;
        }
        let info = info_dir.join(format!("{stored}.trashinfo"));
        if let Err(e) = fs::write(&info, trashinfo_contents(path, time)) {
            out.push((path.clone(), DeleteOutcome::Failed(e.to_string())));
            continue;
        }
        out.push((path.clone(), DeleteOutcome::Trashed(dest)));
    }
    out
}
