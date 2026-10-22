use std::path::{Path, PathBuf};

use crate::edit::EditOp;

/// One recorded edit: the ops list before the change and after it, for a specific file.
///
/// Storing both sides (instead of an "inverse op") keeps the semantics exact: `Crop` has no inverse
/// that can be derived from the op alone, and the ops list is what the UI actually renders.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub path: PathBuf,
    pub ops_before: Vec<EditOp>,
    pub ops_after: Vec<EditOp>,
}

impl Change {
    pub fn new(path: PathBuf, ops_before: Vec<EditOp>, op: EditOp) -> Change {
        let mut ops_after = ops_before.clone();
        ops_after.push(op);
        Change {
            path,
            ops_before,
            ops_after,
        }
    }
}

/// Standard two-stack undo history: a new change clears the redo stack, undo moves a change to redo,
/// redo moves it back to the undo stack.
#[derive(Debug, Default, Clone)]
pub struct UndoStack {
    undo: Vec<Change>,
    redo: Vec<Change>,
}

impl UndoStack {
    pub fn new() -> UndoStack {
        UndoStack::default()
    }

    /// Record a change. A no-op (before == after) is not recorded, so it cannot pollute history.
    /// Returns false when nothing was recorded.
    pub fn record(&mut self, change: Change) -> bool {
        if change.ops_before == change.ops_after {
            return false;
        }
        self.undo.push(change);
        self.redo.clear();
        true
    }

    /// Apply `op` to `ops_before` for `path` and record the change; returns the new ops list.
    pub fn apply(&mut self, path: &Path, ops_before: &[EditOp], op: EditOp) -> Vec<EditOp> {
        let change = Change::new(path.to_path_buf(), ops_before.to_vec(), op);
        let after = change.ops_after.clone();
        self.record(change);
        after
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Pop the newest change; the caller restores `ops_before`. The change moves to the redo stack.
    pub fn undo(&mut self) -> Option<Change> {
        let change = self.undo.pop()?;
        self.redo.push(change.clone());
        Some(change)
    }

    /// Pop the newest undone change; the caller re-applies `ops_after`. It moves back to undo.
    pub fn redo(&mut self) -> Option<Change> {
        let change = self.redo.pop()?;
        self.undo.push(change.clone());
        Some(change)
    }

    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }

    pub fn is_empty(&self) -> bool {
        self.undo.is_empty() && self.redo.is_empty()
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}
