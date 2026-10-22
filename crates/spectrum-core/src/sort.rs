//! Display order for the list/grid. `scan_folder` gives a name-sorted baseline; the UI can
//! reorder without re-scanning, so sorting returns indices rather than cloned entries.

use std::cmp::Ordering;
use std::time::SystemTime;

use crate::hdr::is_hdr_signal;
use crate::scan::{MediaEntry, MediaKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Name,
    /// Newest first.
    DateModified,
    /// Photos before videos.
    Kind,
    /// HDR/signalled files first.
    Transfer,
    /// Largest file first.
    Size,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortDirection {
    /// The key's natural order (Name A→Z, Date newest first, Size largest first, ...).
    #[default]
    Natural,
    /// Everything flipped, including the name tie-break.
    Reversed,
}

impl SortDirection {
    pub fn label(&self) -> &'static str {
        match self {
            SortDirection::Natural => "natural",
            SortDirection::Reversed => "reversed",
        }
    }

    /// The name `sort.set` accepts.
    pub fn parse(name: &str) -> Option<SortDirection> {
        match name {
            "natural" => Some(SortDirection::Natural),
            "reversed" => Some(SortDirection::Reversed),
            _ => None,
        }
    }
}

impl SortKey {
    /// The name `sort.set` accepts.
    pub fn parse(name: &str) -> Option<SortKey> {
        match name {
            "name" => Some(SortKey::Name),
            "date" => Some(SortKey::DateModified),
            "kind" => Some(SortKey::Kind),
            "transfer" => Some(SortKey::Transfer),
            "size" => Some(SortKey::Size),
            _ => None,
        }
    }

    /// The argument string for this key (round-trips through `parse`).
    pub fn arg(&self) -> &'static str {
        match self {
            SortKey::Name => "name",
            SortKey::DateModified => "date",
            SortKey::Kind => "kind",
            SortKey::Transfer => "transfer",
            SortKey::Size => "size",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            SortKey::Name => "Name",
            SortKey::DateModified => "Date",
            SortKey::Kind => "Kind",
            SortKey::Transfer => "HDR first",
            SortKey::Size => "Size",
        }
    }
}

fn kind_rank(kind: MediaKind) -> u8 {
    match kind {
        MediaKind::Photo => 0,
        MediaKind::Video => 1,
    }
}

fn transfer_rank(e: &MediaEntry) -> u8 {
    if e.signal.as_ref().is_some_and(is_hdr_signal) {
        0
    } else {
        1
    }
}

/// Indices into `entries` in display order. Every comparison falls back to the lowercased file
/// name, so the result is stable and reproducible regardless of `read_dir` order.
pub fn sort_entries(entries: &[MediaEntry], key: SortKey) -> Vec<usize> {
    sort_entries_dir(entries, key, SortDirection::Natural)
}

pub fn sort_entries_dir(entries: &[MediaEntry], key: SortKey, dir: SortDirection) -> Vec<usize> {
    let all: Vec<usize> = (0..entries.len()).collect();
    sort_indices(entries, key, &all, dir)
}

/// Same ordering, restricted to a subset (used after filtering).
pub fn sort_indices(
    entries: &[MediaEntry],
    key: SortKey,
    idx: &[usize],
    dir: SortDirection,
) -> Vec<usize> {
    let mut idx = idx.to_vec();
    idx.sort_by(|&a, &b| {
        let ea = &entries[a];
        let eb = &entries[b];
        let primary = match key {
            SortKey::Name => Ordering::Equal,
            SortKey::DateModified => eb
                .mtime
                .unwrap_or(SystemTime::UNIX_EPOCH)
                .cmp(&ea.mtime.unwrap_or(SystemTime::UNIX_EPOCH)),
            SortKey::Kind => kind_rank(ea.kind).cmp(&kind_rank(eb.kind)),
            SortKey::Transfer => transfer_rank(ea).cmp(&transfer_rank(eb)),
            SortKey::Size => eb.size.cmp(&ea.size),
        };
        let full = primary.then_with(|| ea.name().to_lowercase().cmp(&eb.name().to_lowercase()));
        if dir == SortDirection::Reversed {
            full.reverse()
        } else {
            full
        }
    });
    idx
}
