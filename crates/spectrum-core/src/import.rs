use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::jpeg;
use crate::scan::kind_for;
use crate::timeline::civil_from_days;

/// EXIF `DateTimeOriginal` for a JPEG file, or None when the file has no EXIF date.
fn exif_date_key(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_ascii_lowercase();
    if ext != "jpg" && ext != "jpeg" {
        return None;
    }
    let data = std::fs::read(path).ok()?;
    jpeg::parse_jpeg(&data)?.exif.date_key()
}

/// `YYYY-MM-DD` (UTC calendar day) used as the import group key.
///
/// The reference groups by EXIF date; EXIF `DateTimeOriginal` is not parsed yet, so this uses the
/// file modification time, which is what `sort::SortKey::DateModified` already uses.
pub fn date_key(t: SystemTime) -> String {
    let days = t
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (y, m, d) = civil_from_days(days / 86400);
    format!("{y:04}-{m:02}-{d:02}")
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportItem {
    pub path: PathBuf,
    pub date_key: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportGroup {
    pub key: String,
    /// Indices into `ImportModel::items`, in display order.
    pub indices: Vec<usize>,
    pub expanded: bool,
}

/// A row the import panel renders: a group header, or one item inside an expanded group.
#[derive(Debug, Clone, PartialEq)]
pub enum ImportRow {
    Header(usize),
    Item(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportModel {
    pub items: Vec<ImportItem>,
    pub groups: Vec<ImportGroup>,
    pub expand_all: bool,
}

impl Default for ImportModel {
    fn default() -> ImportModel {
        ImportModel {
            items: Vec::new(),
            groups: Vec::new(),
            expand_all: true,
        }
    }
}

impl ImportModel {
    /// Scan media files and group them by day, newest group first. Inside a group items are sorted
    /// by name so the panel is deterministic.
    pub fn scan(dir: &Path) -> ImportModel {
        // (path, group key, mtime) — the key is the EXIF date when the file has one.
        let mut entries: Vec<(PathBuf, String, SystemTime)> = Vec::new();
        if let Ok(read) = std::fs::read_dir(dir) {
            for item in read.flatten() {
                let path = item.path();
                if kind_for(&path).is_none() {
                    continue;
                }
                let mtime = item.metadata().ok().and_then(|m| m.modified().ok());
                // EXIF DateTimeOriginal wins when present (the reference groups by EXIF date);
                // otherwise fall back to the file mtime.
                let key = exif_date_key(&path)
                    .unwrap_or_else(|| date_key(mtime.unwrap_or(SystemTime::UNIX_EPOCH)));
                entries.push((path, key, mtime.unwrap_or(SystemTime::UNIX_EPOCH)));
            }
        }

        // newest day first; within a day, name ascending
        entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

        let mut model = ImportModel::default();
        for (path, key, _) in &entries {
            let key = key.clone();
            let idx = model.items.len();
            model.items.push(ImportItem {
                path: path.clone(),
                date_key: key.clone(),
            });
            match model.groups.iter_mut().find(|g| g.key == key) {
                Some(g) => g.indices.push(idx),
                None => model.groups.push(ImportGroup {
                    key,
                    indices: vec![idx],
                    expanded: model.expand_all,
                }),
            }
        }
        model
    }

    pub fn toggle(&mut self, group: usize) {
        if let Some(g) = self.groups.get_mut(group) {
            g.expanded = !g.expanded;
        }
    }

    pub fn set_all_expanded(&mut self, expanded: bool) {
        self.expand_all = expanded;
        for g in self.groups.iter_mut() {
            g.expanded = expanded;
        }
    }

    /// Rows in display order: header, then the items of every expanded group.
    pub fn rows(&self) -> Vec<ImportRow> {
        let mut rows = Vec::new();
        for (gi, g) in self.groups.iter().enumerate() {
            rows.push(ImportRow::Header(gi));
            if g.expanded {
                for &i in &g.indices {
                    rows.push(ImportRow::Item(i));
                }
            }
        }
        rows
    }

    pub fn group_label(&self, group: usize) -> String {
        match self.groups.get(group) {
            Some(g) => format!("{}  ·  {}", g.key, g.indices.len()),
            None => String::new(),
        }
    }

    /// Remove items that have been moved elsewhere, then rebuild the groups (an emptied group is
    /// dropped so the panel does not show a `0` header).
    pub fn remove_items(&mut self, paths: &[PathBuf]) {
        self.items.retain(|i| !paths.contains(&i.path));
        // Keep each group's expansion state by key: rebuilding in item order would otherwise shift
        // the flags onto the wrong groups.
        let expanded: Vec<(String, bool)> = self
            .groups
            .iter()
            .map(|g| (g.key.clone(), g.expanded))
            .collect();
        let keys: Vec<String> = self.items.iter().map(|i| i.date_key.clone()).collect();
        self.groups.clear();
        for (idx, key) in keys.iter().enumerate() {
            match self.groups.iter_mut().find(|g| g.key == *key) {
                Some(g) => g.indices.push(idx),
                None => {
                    let was_expanded = expanded
                        .iter()
                        .find(|(k, _)| k == key)
                        .map(|(_, e)| *e)
                        .unwrap_or(self.expand_all);
                    self.groups.push(ImportGroup {
                        key: key.clone(),
                        indices: vec![idx],
                        expanded: was_expanded,
                    });
                }
            }
        }
    }

    pub fn item_name(&self, idx: usize) -> String {
        self.items
            .get(idx)
            .and_then(|i| i.path.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    }
}
