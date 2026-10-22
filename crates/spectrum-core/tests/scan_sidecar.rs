//! `scan_folder` must pick up the edit stored in the XMP sidecar.

use spectrum_core::{crop::CropRect, edit::EditOp, xmp};
use spectrum_testkit::fixture;

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("spectrum-core-{tag}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn scan_reads_sidecar_edit() {
    let dir = temp_dir("scan_sidecar");
    let path = fixture::write_jpeg_fixture(&dir, "a.jpg", None, Some(1), None, 1);
    xmp::delete_sidecar(&path);

    // No sidecar yet -> None.
    let entries = spectrum_core::scan::scan_folder(&dir);
    assert_eq!(entries.len(), 1);
    assert!(entries[0].edit.is_none());

    let edit = spectrum_core::edit::CompositeEdit::from(&[
        EditOp::Rotate(90),
        EditOp::Crop(CropRect::new(0.25, 0.25, 0.5, 0.5)),
    ]);
    xmp::write_sidecar(&path, &edit, 1, None).unwrap();

    let entries = spectrum_core::scan::scan_folder(&dir);
    let sd = entries[0].edit.as_ref().expect("sidecar read back");
    assert_eq!(sd.rotation, 90);
    assert!(!sd.flip_h);
    assert_eq!(sd.crop, Some(CropRect::new(0.25, 0.25, 0.5, 0.5)));
    // SidecarData converts to the same CompositeEdit the display pipeline uses.
    assert_eq!(sd.edit(), edit);
}

#[test]
fn sidecar_only_when_the_edit_is_not_identity() {
    let dir = temp_dir("scan_sidecar_identity");
    let path = fixture::write_jpeg_fixture(&dir, "a.jpg", None, Some(1), None, 1);
    xmp::delete_sidecar(&path);
    xmp::write_sidecar(
        &path,
        &spectrum_core::edit::CompositeEdit::default(),
        1,
        None,
    )
    .unwrap();

    // An empty sidecar (no crop, rotation 0) is not an edit.
    let entries = spectrum_core::scan::scan_folder(&dir);
    assert!(entries[0].edit.is_none());
}
