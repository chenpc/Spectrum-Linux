//! Folder scan: list media files and read their colour signalling.

use std::path::{Path, PathBuf};

use crate::hdr::ImageSignal;
use crate::heif::{find_colr, find_icc_profiles, parse_colr, Nclx};
use crate::jpeg::parse_jpeg;
use crate::mp4::VideoInfo;
use crate::xmp::{self, SidecarData};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Photo,
    Video,
}

impl MediaKind {
    /// The name `filter.kind` accepts.
    pub fn parse(name: &str) -> Option<MediaKind> {
        match name {
            "photo" => Some(MediaKind::Photo),
            "video" => Some(MediaKind::Video),
            _ => None,
        }
    }

    /// The argument string for this kind (round-trips through `parse`).
    pub fn arg(&self) -> &'static str {
        self.label()
    }

    pub fn label(&self) -> &'static str {
        match self {
            MediaKind::Photo => "photo",
            MediaKind::Video => "video",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct MediaEntry {
    pub path: PathBuf,
    pub kind: MediaKind,
    pub signal: Option<ImageSignal>,
    /// Non-destructive edit stored in `{name}.{ext}.xmp` (None = no sidecar / identity edit).
    pub edit: Option<SidecarData>,
    /// Codec fourcc + duration for video files (None when the file has neither).
    pub video: Option<VideoInfo>,
    /// File modification time, used by `sort::SortKey::DateModified`.
    pub mtime: Option<std::time::SystemTime>,
    /// File size in bytes, used by `sort::SortKey::Size`.
    pub size: u64,
}

pub fn kind_for(path: &Path) -> Option<MediaKind> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    match ext.as_str() {
        "heic" | "heif" | "hif" => Some(MediaKind::Photo),
        "jpg" | "jpeg" => Some(MediaKind::Photo),
        "png" | "webp" => Some(MediaKind::Photo),
        "mp4" | "mov" => Some(MediaKind::Video),
        _ => None,
    }
}

impl MediaEntry {
    /// Display name used as the accessible label in the grid.
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| self.path.display().to_string())
    }
}

/// Read `nclx` from a HEIF file (kept for the HEIF-specific tests).
pub fn detect_nclx(path: &Path) -> Option<Nclx> {
    let data = std::fs::read(path).ok()?;
    let colr = find_colr(&data)?;
    parse_colr(colr)
}

/// Read an ICC profile from a HEIF file (kept for the HEIF-specific tests).
pub fn detect_icc(path: &Path) -> Option<crate::icc::IccInfo> {
    let data = std::fs::read(path).ok()?;
    crate::icc::pick_icc(&find_icc_profiles(&data))
}

/// Colour signalling for one file, from the header only (no pixel decode).
///
/// HEIF/HEIC and MP4/MOV use the ISO 14496-12 `colr`/`nclx` box; JPEG uses EXIF + the ICC profile
/// description. PNG/WebP have no transfer-function signalling here, so they return None.
pub fn detect_signal(path: &Path) -> Option<ImageSignal> {
    let ext = path.extension()?.to_string_lossy().to_ascii_lowercase();
    let data = std::fs::read(path).ok()?;
    match ext.as_str() {
        "jpg" | "jpeg" => parse_jpeg(&data).map(|info| crate::hdr::signal_for_jpeg(&info)),
        "heic" | "heif" | "hif" | "mp4" | "mov" => {
            let nclx = find_colr(&data).and_then(parse_colr);
            let icc = crate::icc::pick_icc(&find_icc_profiles(&data));
            crate::hdr::signal_for_heif(nclx.as_ref(), icc.as_ref())
        }
        _ => None,
    }
}

/// Scan a folder (non-recursive) and sort by file name so tests are deterministic.
pub fn scan_folder(dir: &Path) -> Vec<MediaEntry> {
    let mut entries: Vec<MediaEntry> = Vec::new();
    for item in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = item.path();
        if !path.is_file() {
            continue;
        }
        let Some(kind) = kind_for(&path) else {
            continue;
        };
        let signal = detect_signal(&path);
        let orientation = signal.as_ref().map(|s| s.orientation as i32).unwrap_or(1);
        let edit = xmp::read_sidecar(&path, orientation);
        let video = if kind == MediaKind::Video {
            std::fs::read(&path)
                .ok()
                .and_then(|d| crate::mp4::parse_video_info(&d))
        } else {
            None
        };
        let meta = item.metadata().ok();
        let mtime = meta.as_ref().and_then(|m| m.modified().ok());
        let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
        entries.push(MediaEntry {
            path,
            kind,
            signal,
            edit,
            video,
            mtime,
            size,
        });
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    entries
}
