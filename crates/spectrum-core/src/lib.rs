#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod clipboard;
pub mod command;
pub mod cover;
pub mod crop;
pub mod edit;
pub mod filter;
pub mod folder_cache;
pub mod hdr;
pub mod heif;
pub mod icc;
pub mod import;
pub mod jpeg;
pub mod library_store;
pub mod model;
pub mod mp4;
pub mod nav;
pub mod orientation;
pub mod scan;
pub mod search;
pub mod select;
pub mod sort;
pub mod status_bar;
pub mod thumbnail;
pub mod timeline;
pub mod trash;
pub mod undo;
pub mod xmp;

pub use clipboard::{paste, rename_dir, set_clipboard, Clipboard, ClipboardMode, PasteOutcome};
pub use command::{describe, dispatch, lookup, specs, CommandError, CommandSpec};
pub use cover::{
    first_cover, list_subfolders, sort_by_cover, subfolders_are_in_order, SubfolderInfo,
};
pub use crop::CropRect;
pub use edit::{CompositeEdit, EditOp};
pub use filter::Filter;
pub use hdr::{
    badge_label, display_mode, icc_signal, is_bt2020_primaries, is_hdr, is_hdr_signal,
    signal_for_heif, signal_for_jpeg, transfer_from_code, transfer_from_profile_name, DisplayMode,
    ImageSignal, SignalSource, TransferFunction,
};
pub use heif::{find_icc, find_icc_profiles, Nclx};
pub use icc::{pick_icc, IccInfo};
pub use jpeg::{ExifData, GainMap, JpegInfo};
pub use library_store::{
    apply as apply_library_store, default_path, load as load_library_store, path_in,
    save as save_library_store, Saved, LIBRARY_STORE_VERSION,
};
pub use model::{compose, FolderSlot, InspectorTab, Library, ViewMode};
pub use mp4::VideoInfo;
pub use nav::{next_index, row_col, Direction};
pub use scan::{
    detect_icc, detect_nclx, detect_signal, kind_for, scan_folder, MediaEntry, MediaKind,
};
pub use select::{apply_click, ClickMods, Selection};
pub use sort::{sort_entries, SortKey};
pub use timeline::{group_by_month, group_label, month_key, MonthGroup, UNKNOWN_GROUP};
pub use trash::{default_trash_root, delete_paths, trashinfo_contents, DeleteOutcome};
pub use xmp::SidecarData;
