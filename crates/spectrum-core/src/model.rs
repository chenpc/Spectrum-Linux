#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

//! The app's model: everything the UI shows and every command mutates lives here, not in the UI.
//!
//! `Library` is UI-agnostic (no egui, no GTK): the UI reads it, commands write it, and tests drive
//! it through [`crate::command::dispatch`] — the same path the user's clicks take.

use std::path::{Path, PathBuf};

use crate::command::CommandError;
use crate::edit::{CompositeEdit, EditOp};
use crate::filter::{filter_entries, Filter};
use crate::hdr::{badge_label, display_mode, DisplayMode};
use crate::scan::{scan_folder, MediaEntry};
use crate::select::{apply_click, ClickMods, Selection};
use crate::sort::{sort_indices, SortDirection, SortKey};
use crate::status_bar::StatusBarModel;
use crate::timeline::{group_by_month, MonthGroup};
use crate::trash::{default_trash_root, delete_paths, DeleteOutcome};
use crate::xmp::{read_sidecar, write_sidecar};

/// What the main area shows: everything, by month, or by year.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    #[default]
    All,
    Months,
    Years,
}

impl ViewMode {
    pub fn label(self) -> &'static str {
        match self {
            ViewMode::All => "All Photos",
            ViewMode::Months => "Months",
            ViewMode::Years => "Years",
        }
    }

    /// The argument string for this view (round-trips through `parse`).
    pub fn arg(self) -> &'static str {
        match self {
            ViewMode::All => "all",
            ViewMode::Months => "months",
            ViewMode::Years => "years",
        }
    }

    pub fn parse(name: &str) -> Option<ViewMode> {
        match name {
            "all" => Some(ViewMode::All),
            "months" => Some(ViewMode::Months),
            "years" => Some(ViewMode::Years),
            _ => None,
        }
    }
}

/// Which inspector panel is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InspectorTab {
    #[default]
    Adjust,
    Info,
}

impl InspectorTab {
    pub fn label(self) -> &'static str {
        match self {
            InspectorTab::Adjust => "Adjust",
            InspectorTab::Info => "Info",
        }
    }

    /// The argument string for this tab (round-trips through `parse`).
    pub fn arg(self) -> &'static str {
        match self {
            InspectorTab::Adjust => "adjust",
            InspectorTab::Info => "info",
        }
    }

    pub fn parse(name: &str) -> Option<InspectorTab> {
        match name {
            "adjust" => Some(InspectorTab::Adjust),
            "info" => Some(InspectorTab::Info),
            _ => None,
        }
    }
}

/// One folder in the sidebar list. `count` is known once that folder has been scanned (the active
/// one always is; the others keep the count they had when they were last opened).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderSlot {
    pub path: PathBuf,
    pub count: Option<usize>,
}

impl FolderSlot {
    /// The basename the sidebar shows, or the whole path when the folder is the filesystem root.
    pub fn label(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.path.display().to_string())
    }
}

#[derive(Debug)]
pub struct Library {
    /// The folders the user added, in sidebar order.
    pub folders: Vec<FolderSlot>,
    /// Index into `folders`; the folder `entries` was read from.
    pub active: usize,
    pub entries: Vec<MediaEntry>,
    pub selection: Selection,
    pub view: ViewMode,
    pub tab: InspectorTab,
    /// Off = tone-map everything to SDR; on = files whose signalling is HDR show in HDR.
    pub hdr_enabled: bool,
    pub filter: Filter,
    /// `YYYY` limit applied after `filter` (set from the Years view); None = any year.
    pub year: Option<String>,
    pub sort: SortKey,
    pub direction: SortDirection,
    pub status: StatusBarModel,
    /// Month group keys the user collapsed.
    pub collapsed: Vec<String>,
    /// The trash directory; overridable so tests never touch the real trash.
    pub trash_root: PathBuf,
}

impl Library {
    pub fn new(folder: &Path) -> Self {
        let mut lib = Library {
            folders: vec![FolderSlot {
                path: folder.to_path_buf(),
                count: None,
            }],
            active: 0,
            entries: Vec::new(),
            selection: Selection::default(),
            view: ViewMode::default(),
            tab: InspectorTab::default(),
            hdr_enabled: true,
            filter: Filter::default(),
            year: None,
            sort: SortKey::DateModified,
            direction: SortDirection::default(),
            status: StatusBarModel::new(),
            collapsed: Vec::new(),
            trash_root: default_trash_root(),
        };
        lib.rescan();
        lib
    }

    /// The folder `entries` came from; an empty list yields an empty path rather than a panic.
    pub fn folder(&self) -> &Path {
        match self.folders.get(self.active) {
            Some(slot) => slot.path.as_path(),
            None => Path::new(""),
        }
    }

    /// Add a folder to the sidebar list and open it. Fails when it is not a directory or is
    /// already in the list, so a mistyped path is reported instead of silently ignored.
    pub fn add_folder(&mut self, path: &Path) -> Result<usize, CommandError> {
        if !path.is_dir() {
            return Err(CommandError::Failed(format!(
                "{} is not a directory",
                path.display()
            )));
        }
        if self.folders.iter().any(|slot| slot.path == path) {
            return Err(CommandError::BadParam(format!(
                "{} is already in the library",
                path.display()
            )));
        }
        self.folders.push(FolderSlot {
            path: path.to_path_buf(),
            count: None,
        });
        let index = self.folders.len() - 1;
        self.open_folder(index)
            .map(|()| index)
            .map_err(|_| CommandError::Failed(format!("{} changed while adding", path.display())))
    }

    /// Open a folder from the list: rescan it and drop the selection it no longer has.
    pub fn open_folder(&mut self, index: usize) -> Result<(), CommandError> {
        if index >= self.folders.len() {
            return Err(CommandError::BadParam(format!(
                "folder index {index} is past the {} folders",
                self.folders.len()
            )));
        }
        self.active = index;
        self.rescan();
        Ok(())
    }

    /// Drop a folder from the library list. The files on disk are untouched.
    pub fn remove_folder(&mut self, index: usize) -> Result<(), CommandError> {
        if index >= self.folders.len() {
            return Err(CommandError::BadParam(format!(
                "folder index {index} is past the {} folders",
                self.folders.len()
            )));
        }
        self.folders.remove(index);
        self.active = self.active.min(self.folders.len().saturating_sub(1));
        self.rescan();
        Ok(())
    }

    /// Re-read the folder. Selection keeps the indices it still has; the design keeps the scroll
    /// position, so a rescan must not silently reset what the user picked.
    pub fn rescan(&mut self) {
        self.entries = scan_folder(self.folder());
        if let Some(slot) = self.folders.get_mut(self.active) {
            slot.count = Some(self.entries.len());
        }
        self.selection.set.retain(|&i| i < self.entries.len());
        self.selection.primary = self.selection.primary.filter(|&i| i < self.entries.len());
    }

    /// Display order: filtered, then sorted, then year-limited.
    pub fn order(&self) -> Vec<usize> {
        let filtered = filter_entries(&self.entries, &self.filter);
        let sorted = sort_indices(&self.entries, self.sort, &filtered, self.direction);
        match &self.year {
            Some(year) => sorted
                .into_iter()
                .filter(|&i| {
                    self.entries[i].mtime.map(month_prefix_of).as_deref() == Some(year.as_str())
                })
                .collect(),
            None => sorted,
        }
    }

    /// Indices the main area lists right now: groups honoured, collapsed groups contribute nothing.
    pub fn visible(&self) -> Vec<usize> {
        match self.view {
            ViewMode::All => group_by_month(&self.entries, &self.order())
                .into_iter()
                .filter(|g| !self.collapsed.contains(&g.key))
                .flat_map(|g| g.indices)
                .collect(),
            ViewMode::Months | ViewMode::Years => self.order(),
        }
    }

    /// Month groups in display order (used by the Library grid and the Months view).
    pub fn groups(&self) -> Vec<MonthGroup> {
        group_by_month(&self.entries, &self.order())
    }

    /// Year keys in display order, newest first, with the indices under each.
    pub fn years(&self) -> Vec<(String, Vec<usize>)> {
        let mut out: Vec<(String, Vec<usize>)> = Vec::new();
        for &i in &self.order() {
            let key = self.entries[i]
                .mtime
                .map(month_prefix_of)
                .unwrap_or_else(|| crate::timeline::UNKNOWN_GROUP.to_string());
            match out.iter_mut().find(|(k, _)| *k == key) {
                Some(g) => g.1.push(i),
                None => out.push((key, vec![i])),
            }
        }
        out
    }

    pub fn toggle_group(&mut self, key: &str) {
        match self.collapsed.iter().position(|k| k == key) {
            Some(at) => {
                self.collapsed.remove(at);
            }
            None => self.collapsed.push(key.to_string()),
        }
    }

    pub fn collapse_all(&mut self) {
        self.collapsed = self.groups().into_iter().map(|g| g.key).collect();
    }

    pub fn expand_all(&mut self) {
        self.collapsed.clear();
    }

    pub fn click(&mut self, index: usize, mods: ClickMods) {
        self.selection = apply_click(&self.selection, index, mods);
    }

    pub fn clear_selection(&mut self) {
        self.selection = Selection::default();
    }

    pub fn select(&mut self, indices: &[usize]) {
        let mut set: Vec<usize> = indices
            .iter()
            .copied()
            .filter(|&i| i < self.entries.len())
            .collect();
        set.sort_unstable();
        set.dedup();
        self.selection.primary = set.last().copied();
        self.selection.set = set;
    }

    pub fn mode_for(&self, index: usize) -> DisplayMode {
        self.entries
            .get(index)
            .map(|e| display_mode(e.signal.as_ref(), self.hdr_enabled))
            .unwrap_or(DisplayMode::Sdr)
    }

    pub fn badge_for(&self, index: usize) -> &'static str {
        self.entries
            .get(index)
            .map(|e| badge_label(e.signal.as_ref(), self.hdr_enabled))
            .unwrap_or("SDR")
    }

    /// The single selected entry, for commands that need exactly one target.
    pub fn sole(&self) -> Option<usize> {
        match self.selection.set.len() {
            1 => self
                .selection
                .set
                .first()
                .copied()
                .filter(|&i| i < self.entries.len()),
            _ => None,
        }
    }

    pub fn selected_paths(&self) -> Vec<PathBuf> {
        self.selection
            .set
            .iter()
            .filter_map(|&i| self.entries.get(i))
            .map(|e| e.path.clone())
            .collect()
    }

    /// The file's own orientation: JPEG's EXIF tag, otherwise 1. The same value is used when
    /// writing and reading a sidecar, so the composition round-trips.
    pub fn orientation_of(&self, index: usize) -> u16 {
        let Some(entry) = self.entries.get(index) else {
            return 1;
        };
        let Ok(data) = std::fs::read(&entry.path) else {
            return 1;
        };
        crate::jpeg::parse_jpeg(&data)
            .map(|info| info.orientation)
            .unwrap_or(1)
    }

    /// The edit currently stored in the sidecar, or an identity edit.
    pub fn edit_of(&self, index: usize) -> CompositeEdit {
        let Some(entry) = self.entries.get(index) else {
            return CompositeEdit {
                rotation: 0,
                flip_h: false,
                crop: None,
            };
        };
        match read_sidecar(&entry.path, self.orientation_of(index) as i32) {
            Some(data) => CompositeEdit {
                rotation: data.rotation,
                flip_h: data.flip_h,
                crop: data.crop,
            },
            None => CompositeEdit {
                rotation: 0,
                flip_h: false,
                crop: None,
            },
        }
    }

    /// Apply one op to the selected photo and store it in the XMP sidecar (never touches the file).
    pub fn apply_edit(&mut self, index: usize, op: EditOp) -> Result<CompositeEdit, CommandError> {
        let entry = self
            .entries
            .get(index)
            .ok_or_else(|| CommandError::BadParam(format!("no entry {index}")))?;
        let orientation = self.orientation_of(index);
        let current = self.edit_of(index);
        let next = compose(current, op);
        write_sidecar(&entry.path, &next, orientation as i32, None).map_err(|e| {
            CommandError::Failed(format!("write sidecar {}: {e}", entry.path.display()))
        })?;
        self.entries[index].edit = read_sidecar(&entry.path, orientation as i32);
        Ok(next)
    }

    /// Drop the sidecar: back to the original file.
    pub fn restore(&mut self, index: usize) -> Result<(), CommandError> {
        let entry = self
            .entries
            .get(index)
            .ok_or_else(|| CommandError::BadParam(format!("no entry {index}")))?;
        crate::xmp::delete_sidecar(&entry.path);
        self.entries[index].edit = None;
        Ok(())
    }

    /// Move the selected files to the trash directory (never deletes in place).
    pub fn trash_selected(
        &mut self,
        now: std::time::SystemTime,
    ) -> Result<Vec<(PathBuf, DeleteOutcome)>, CommandError> {
        let paths = self.selected_paths();
        if paths.is_empty() {
            return Err(CommandError::Disabled("nothing is selected".to_string()));
        }
        self.status.begin("Trash");
        self.status.update(paths.len(), "files");
        let outcomes = delete_paths(&paths, &self.trash_root, &crate::trash::iso8601_utc(now));
        let moved = outcomes
            .iter()
            .filter(|(_, o)| matches!(o, DeleteOutcome::Trashed(_)))
            .count();
        self.status
            .finish(&format!("Trashed {moved} of {}", paths.len()), now);
        self.clear_selection();
        self.rescan();
        Ok(outcomes)
    }
}

/// `YYYY-MM` -> `YYYY`; unknown mtime keeps the `unknown` key.
fn month_prefix_of(t: std::time::SystemTime) -> String {
    crate::timeline::month_key(t).chars().take(4).collect()
}

/// Apply one edit op to an existing composite edit, matching `CompositeEdit::from` semantics.
pub fn compose(current: CompositeEdit, op: EditOp) -> CompositeEdit {
    match op {
        EditOp::Rotate(deg) => CompositeEdit {
            rotation: (current.rotation + deg % 360 + 360) % 360,
            flip_h: current.flip_h,
            crop: current.crop.map(|c| c.rotated(deg)),
        },
        EditOp::FlipH => CompositeEdit {
            rotation: (360 - current.rotation) % 360,
            flip_h: !current.flip_h,
            crop: current.crop.map(|c| c.flipped_h()),
        },
        EditOp::Crop(rect) => CompositeEdit {
            rotation: current.rotation,
            flip_h: current.flip_h,
            crop: Some(rect),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compose_matches_the_op_list() {
        let crop = crate::crop::CropRect::new(0.1, 0.2, 0.3, 0.4);
        let via_ops = CompositeEdit::from(&[EditOp::Rotate(90), EditOp::FlipH, EditOp::Crop(crop)]);
        let mut built = CompositeEdit {
            rotation: 0,
            flip_h: false,
            crop: None,
        };
        for op in [EditOp::Rotate(90), EditOp::FlipH, EditOp::Crop(crop)] {
            built = compose(built, op);
        }
        assert_eq!(built.rotation, via_ops.rotation);
        assert_eq!(built.flip_h, via_ops.flip_h);
        assert_eq!(built.crop, via_ops.crop);
    }

    #[test]
    fn view_and_tab_parse_their_names() {
        assert_eq!(ViewMode::parse("months"), Some(ViewMode::Months));
        assert_eq!(ViewMode::parse("nope"), None);
        assert_eq!(InspectorTab::parse("info"), Some(InspectorTab::Info));
        assert_eq!(ViewMode::Years.label(), "Years");
    }

    #[test]
    fn month_prefix_of_truncates_and_unknown_survives() {
        use std::time::{Duration, UNIX_EPOCH};
        assert_eq!(
            month_prefix_of(UNIX_EPOCH + Duration::from_secs(1_700_000_000)),
            "2023"
        );
        assert_eq!(
            crate::timeline::month_key(UNIX_EPOCH)
                .chars()
                .take(4)
                .collect::<String>(),
            "1970"
        );
    }
}
