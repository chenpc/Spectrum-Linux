//! Port of macOS `FolderListCache` (DESIGN.md §4.10): in-memory subfolder listings plus the
//! session-level "already scanned this session" flag that avoids repeat directory I/O.
//!
//! macOS early-returns only when three conditions hold at once: the path was scanned this session,
//! the cache has entries for it, and every cached cover file still exists. A folder monitor event
//! (or a deleted subfolder) calls `invalidate`, which clears both the cache and the scanned flag, so
//! the next call rescans.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::cover::{list_subfolders, sort_by_cover, SubfolderInfo};

#[derive(Debug, Default, Clone)]
pub struct FolderListCache {
    memory: HashMap<PathBuf, Vec<SubfolderInfo>>,
    scanned: HashSet<PathBuf>,
}

impl FolderListCache {
    pub fn new() -> FolderListCache {
        FolderListCache::default()
    }

    pub fn entries(&self, parent: &Path) -> Option<Vec<SubfolderInfo>> {
        self.memory.get(parent).cloned()
    }

    /// Cached entry for one child directory, looked up through its parent.
    pub fn entry_for_child(&self, child: &Path, under: &Path) -> Option<SubfolderInfo> {
        self.memory
            .get(under)?
            .iter()
            .find(|i| i.path == child)
            .cloned()
    }

    pub fn is_scanned(&self, parent: &Path) -> bool {
        self.scanned.contains(parent)
    }

    /// Storing entries also marks the path as scanned this session, exactly like `setEntries`.
    pub fn set_entries(&mut self, parent: &Path, infos: &[SubfolderInfo]) {
        self.memory.insert(parent.to_path_buf(), infos.to_vec());
        self.scanned.insert(parent.to_path_buf());
    }

    pub fn invalidate(&mut self, parent: &Path) {
        self.memory.remove(parent);
        self.scanned.remove(parent);
    }

    pub fn clear(&mut self) {
        self.memory.clear();
        self.scanned.clear();
    }

    /// The three macOS conditions: scanned this session, cache present, every cached cover still on
    /// disk. A cover that no longer exists (subfolder deleted) forces a rescan.
    pub fn can_reuse(&self, parent: &Path) -> bool {
        if !self.is_scanned(parent) {
            return false;
        }
        match self.memory.get(parent) {
            None => false,
            Some(cached) => cached.iter().all(|i| match &i.cover {
                Some(c) => c.exists(),
                None => true,
            }),
        }
    }

    /// Cached listing when it can be reused, otherwise rescan and store. Returns name-sorted entries;
    /// the UI order comes from `sort_by_cover`.
    pub fn list(&mut self, parent: &Path) -> Vec<SubfolderInfo> {
        if self.can_reuse(parent) {
            if let Some(infos) = self.memory.get(parent) {
                return infos.clone();
            }
        }
        let infos = list_subfolders(parent);
        self.set_entries(parent, &infos);
        infos
    }

    /// UI-facing order (newest cover first) taken from the cache.
    pub fn sorted(&mut self, parent: &Path) -> Vec<SubfolderInfo> {
        sort_by_cover(&self.list(parent))
    }
}
