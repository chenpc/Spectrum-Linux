//! Multi-select model for the file list (macOS supports Cmd/Shift selection).
//!
//! The UI keeps its own selection so the semantics are explicit and testable: GTK's native
//! MULTIPLE mode cannot be driven in-process, and its behaviour differs from the app's rules.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClickMods {
    pub ctrl: bool,
    pub shift: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    /// Anchor for a Shift click; the last clicked row.
    pub primary: Option<usize>,
    /// Selected indices, always sorted ascending and unique.
    pub set: Vec<usize>,
}

impl Selection {
    pub fn count(&self) -> usize {
        self.set.len()
    }

    pub fn contains(&self, i: usize) -> bool {
        self.set.contains(&i)
    }
}

/// Apply a row click with the current modifier state.
pub fn apply_click(sel: &Selection, index: usize, mods: ClickMods) -> Selection {
    let mut out = sel.clone();
    // Shift extends from the anchor, so it must not move the anchor; plain and Ctrl clicks do.
    if !mods.shift {
        out.primary = Some(index);
    }
    if mods.shift {
        // Shift extends from the anchor to the clicked row (inclusive) and replaces the set.
        let start = sel.primary.unwrap_or(index);
        let (lo, hi) = if start <= index {
            (start, index)
        } else {
            (index, start)
        };
        out.set = (lo..=hi).collect();
    } else if mods.ctrl {
        if out.set.contains(&index) {
            out.set.retain(|&i| i != index);
        } else {
            out.set.push(index);
            out.set.sort_unstable();
        }
    } else {
        out.set = vec![index];
    }
    out
}
