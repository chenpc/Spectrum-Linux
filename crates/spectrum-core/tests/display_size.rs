//! `CompositeEdit::display_size` — crop lives in the rotated coordinate space, so rotation must be
//! applied to the image size before the crop is resolved.

use spectrum_core::{crop::CropRect, edit::CompositeEdit, EditOp};

#[test]
fn rotation_only_swaps_dimensions() {
    let edit = CompositeEdit::from(&[EditOp::Rotate(90)]);
    assert_eq!(edit.display_size(100, 50), (50, 100));
    assert_eq!(
        CompositeEdit::from(&[EditOp::Rotate(180)]).display_size(100, 50),
        (100, 50)
    );
    assert_eq!(
        CompositeEdit::from(&[EditOp::Rotate(-90)]).display_size(100, 50),
        (50, 100)
    );
}

#[test]
fn crop_only_uses_source_dimensions() {
    let edit = CompositeEdit::from(&[EditOp::Crop(CropRect::new(0.5, 0.5, 0.5, 0.5))]);
    assert_eq!(edit.display_size(100, 50), (50, 25));
}

#[test]
fn crop_after_rotation_uses_rotated_dimensions() {
    // UI draws the crop on the already-rotated image: 100x50 -> 50x100, right half -> 25x100.
    let edit = CompositeEdit::from(&[
        EditOp::Rotate(90),
        EditOp::Crop(CropRect::new(0.5, 0.0, 0.5, 1.0)),
    ]);
    assert_eq!(edit.display_size(100, 50), (25, 100));

    let p = edit.crop.unwrap().pixel_rect(50, 100);
    assert_eq!((p.x, p.y, p.width, p.height), (25.0, 0.0, 25.0, 100.0));
    // the crop rectangle must fit inside the rotated image
    assert!(p.x + p.width <= 50.0 && p.y + p.height <= 100.0);
}

#[test]
fn crop_before_rotation_is_transformed_not_rescaled() {
    // A crop drawn before a rotate stays on the same pixels, so it is remapped (w/h swapped),
    // not re-interpreted against the new image size.
    let edit = CompositeEdit::from(&[
        EditOp::Crop(CropRect::new(0.5, 0.0, 0.5, 1.0)),
        EditOp::Rotate(90),
    ]);
    assert_eq!(edit.crop.unwrap(), CropRect::new(0.0, 0.5, 1.0, 0.5));
    // rotation preserves the crop area: 50x50 on the rotated 50x100 image
    assert_eq!(edit.display_size(100, 50), (50, 50));
}
