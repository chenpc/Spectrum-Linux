//! Subfolder listing with a cover file, mirroring macOS `FolderReader.listSubfolders`.
//!
//! The cover is the first media file in the directory (non-recursive): an image is preferred over
//! any other media, and within each group the lexicographically smallest name wins. macOS does this
//! with an O(n) min-scan instead of sorting the whole listing, because sorting a large directory is
//! measurably slower. Hidden files (name starting with `.`) are skipped.

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::scan::{kind_for, MediaKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubfolderInfo {
    pub name: String,
    pub path: PathBuf,
    pub cover: Option<PathBuf>,
    pub cover_date: Option<SystemTime>,
    pub cover_kind: Option<MediaKind>,
}

/// Cover for one directory: smallest-name image, else smallest-name media. `None` when the folder has
/// no media file at all.
pub fn first_cover(dir: &Path) -> Option<(PathBuf, SystemTime, MediaKind)> {
    let mut best_image: Option<(String, PathBuf, SystemTime)> = None;
    let mut best_media: Option<(String, PathBuf, SystemTime)> = None;

    let items = match std::fs::read_dir(dir) {
        Ok(items) => items,
        Err(_) => return None,
    };
    for item in items {
        let entry = match item {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        let name = match path.file_name() {
            Some(n) => n.to_string_lossy().to_string(),
            None => continue,
        };
        if name.starts_with('.') {
            continue;
        }
        let kind = match kind_for(&path) {
            Some(k) => k,
            None => continue,
        };
        let date = match entry.metadata().ok().and_then(|m| m.modified().ok()) {
            Some(d) => d,
            None => continue,
        };
        let slot = if kind == MediaKind::Photo {
            &mut best_image
        } else {
            &mut best_media
        };
        let replace = slot.as_ref().is_none_or(|(best, _, _)| name < *best);
        if replace {
            *slot = Some((name, path, date));
        }
    }

    let pick = best_image.or(best_media)?;
    let kind = kind_for(&pick.1)?;
    Some((pick.1, pick.2, kind))
}

/// Row label used by the UI: `folder · cover-file`, or `folder · (no cover)`.
pub fn cover_label(info: &SubfolderInfo) -> String {
    match &info.cover {
        Some(p) => format!(
            "{}  ·  {}",
            info.name,
            p.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default()
        ),
        None => format!("{}  ·  (no cover)", info.name),
    }
}

/// macOS `subfoldersAreInOrder`: newest cover date first, name ascending on a tie, folders without a
/// cover last. `list_subfolders` stays name-sorted so the raw listing is deterministic; the UI order
/// comes from `sort_by_cover`.
pub fn subfolders_are_in_order(a: &SubfolderInfo, b: &SubfolderInfo) -> bool {
    match (a.cover_date, b.cover_date) {
        (Some(x), Some(y)) => {
            if x == y {
                a.name < b.name
            } else {
                x > y
            }
        }
        (Some(_), None) => true,
        (None, Some(_)) => false,
        (None, None) => a.name < b.name,
    }
}

pub fn sort_by_cover(infos: &[SubfolderInfo]) -> Vec<SubfolderInfo> {
    let mut out = infos.to_vec();
    out.sort_by(|a, b| {
        if subfolders_are_in_order(a, b) {
            Ordering::Less
        } else if subfolders_are_in_order(b, a) {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    });
    out
}

/// Direct subdirectories of `dir`, sorted by name so the UI order is deterministic.
pub fn list_subfolders(dir: &Path) -> Vec<SubfolderInfo> {
    let mut out = Vec::new();
    for item in match std::fs::read_dir(dir) {
        Ok(items) => items,
        Err(_) => return out,
    } {
        let entry = match item {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = match path.file_name() {
            Some(n) => n.to_string_lossy().to_string(),
            None => continue,
        };
        if name.starts_with('.') {
            continue;
        }
        let cover = first_cover(&path);
        out.push(SubfolderInfo {
            name,
            path: path.clone(),
            cover: cover.as_ref().map(|c| c.0.clone()),
            cover_date: cover.as_ref().map(|c| c.1),
            cover_kind: cover.map(|c| c.2),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}
