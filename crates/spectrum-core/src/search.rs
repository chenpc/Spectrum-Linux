//! Filename + folder-name search, mirroring macOS `SearchResultsView.search` + `walkDirectory`.
//!
//! Rules that matter for behaviour parity:
//! - the query is trimmed and must be at least 2 characters, otherwise nothing is searched
//! - matching is case-insensitive substring matching
//! - the walk is recursive (unlike `scan_folder`) and skips hidden files
//! - photos are capped to the first 200 encountered *in walk order*, then sorted newest-first
//! - folder matches are deduplicated by path

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::scan::{kind_for, MediaKind};

pub const PHOTO_LIMIT: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub path: PathBuf,
    pub name: String,
    pub kind: MediaKind,
    pub mtime: Option<SystemTime>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchResult {
    pub photos: Vec<SearchHit>,
    /// `(path, name)` in walk order.
    pub folders: Vec<(PathBuf, String)>,
}

/// `None` when the query is too short to search (macOS guard: `trimmed.count >= 2`).
pub fn normalize_query(query: &str) -> Option<String> {
    let trimmed = query.trim();
    if trimmed.chars().count() < 2 {
        return None;
    }
    Some(trimmed.to_lowercase())
}

pub fn search(root: &Path, query: &str) -> SearchResult {
    let lower = match normalize_query(query) {
        Some(l) => l,
        None => return SearchResult::default(),
    };
    let mut photos = Vec::new();
    let mut folders = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    walk(root, &lower, &mut photos, &mut folders, &mut seen);

    // Cap first (walk order), then sort - so the cap is over what was encountered, not over the
    // newest 200.
    photos.truncate(PHOTO_LIMIT);
    photos.sort_by_key(|hit| std::cmp::Reverse(hit.mtime));
    SearchResult { photos, folders }
}

fn walk(
    dir: &Path,
    lower: &str,
    photos: &mut Vec<SearchHit>,
    folders: &mut Vec<(PathBuf, String)>,
    seen: &mut HashSet<PathBuf>,
) {
    let items = match std::fs::read_dir(dir) {
        Ok(items) => items,
        Err(_) => return,
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
        if path.is_dir() {
            if name.to_lowercase().contains(lower) && seen.insert(path.clone()) {
                folders.push((path.clone(), name));
            }
            walk(&path, lower, photos, folders, seen);
        } else if let Some(kind) = kind_for(&path) {
            if name.to_lowercase().contains(lower) {
                let mtime = entry.metadata().ok().and_then(|m| m.modified().ok());
                photos.push(SearchHit {
                    path: path.clone(),
                    name,
                    kind,
                    mtime,
                });
            }
        }
    }
}
