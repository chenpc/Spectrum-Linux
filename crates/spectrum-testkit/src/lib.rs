//! Synthetic media fixtures: self-contained HEIF/MP4/JPEG/PNG writers that build files from
//! byte-level structures, so no personal photo or third-party sample lives in this repository.
//!
//! **Dev-dependency only.** Nothing in the app may depend on this crate; the app's own build must
//! not contain fixture code. It has no dependency on `spectrum-core` — the fixtures are raw bytes
//! plus the ISO/IEC codes they encode, so tests can use them without pulling product code in.
//!
//! `fixture.rs` panics when a fixture file cannot be written. That is deliberate and scoped to
//! this crate: a fixture that failed to land on disk would make the test assert against an empty
//! directory, which is a wrong result rather than an obvious one. Product code keeps the
//! no-panic rule (`crates/spectrum-core/src/lib.rs`).
#![allow(clippy::unwrap_used, clippy::expect_used)]
#![forbid(unsafe_code)]

pub mod fixture;
pub use fixture::{
    icc_gain_map, icc_profile, synthetic_heif, synthetic_heif_with_profiles, synthetic_jpeg,
    synthetic_jpeg_exif, synthetic_mp4, synthetic_png, write_fixture, ExifFixture,
};
