//! Reading a directory the way the "Add Folder" browser shows it.
//!
//! The browser only ever offers directories: adding a file to the library is not a thing, and
//! hidden entries (dot-prefixed) are not what someone browsing for photos wants to see. Each listed
//! directory carries how many media files sit directly inside it, which is what makes a folder of
//! 2 000 photos findable at a glance. The count is deliberately shallow — one directory's own files,
//! not its subtree — because a recursive count over a large library is exactly the pause this screen
//! must not cause.

use std::path::{Path, PathBuf};

use crate::scan;

/// One entry in the browser's list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub path: PathBuf,
    /// Media files directly inside this directory; nested directories are not counted.
    pub media: usize,
}

/// Where a newly opened browser starts: the user's home directory, falling back to the current
/// directory when the environment has no `HOME` (a sandbox, a systemd unit with no user).
pub fn home() -> PathBuf {
    match std::env::var_os("HOME") {
        Some(home) if !home.is_empty() => PathBuf::from(home),
        _ => PathBuf::from("."),
    }
}

/// The directory one level up, or `None` at the filesystem root.
pub fn parent(dir: &Path) -> Option<PathBuf> {
    dir.parent().map(|p| p.to_path_buf())
}

/// Immediate subdirectories of `dir`, sorted by name, with the media count of each.
///
/// # Errors
/// A short human-readable reason when `dir` cannot be read: the browser shows it instead of
/// silently rendering an empty list, which would look like an empty folder.
pub fn listing(dir: &Path) -> Result<Vec<DirEntry>, String> {
    let read =
        std::fs::read_dir(dir).map_err(|err| format!("cannot read {}: {err}", dir.display()))?;
    let mut entries: Vec<DirEntry> = Vec::new();
    for child in read {
        let child = match child {
            Ok(child) => child,
            Err(_) => continue,
        };
        let name = child.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        if !child.path().is_dir() {
            continue;
        }
        entries.push(DirEntry {
            media: count_media(&child.path()),
            name,
            path: child.path(),
        });
    }
    entries.sort_by_key(|entry| entry.name.to_lowercase());
    Ok(entries)
}

/// Media files directly inside `dir`. A file that cannot be opened is skipped rather than failing
/// the count: a directory listing must not be held hostage by one unreadable file.
pub fn count_media(dir: &Path) -> usize {
    let Ok(read) = std::fs::read_dir(dir) else {
        return 0;
    };
    read.filter_map(|child| child.ok())
        .filter(|child| scan::kind_for(&child.path()).is_some())
        .count()
}
