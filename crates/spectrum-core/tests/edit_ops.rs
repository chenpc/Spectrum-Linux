use spectrum_core::{CompositeEdit, CropRect, EditOp};

const EPS: f64 = 1e-10;

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() < EPS
}

#[test]
fn crop_rect_rotated_0_and_360_returns_self() {
    let crop = CropRect::new(0.1, 0.2, 0.3, 0.4);
    assert_eq!(crop.rotated(0), crop);
    assert_eq!(crop.rotated(360), crop);
}

#[test]
fn crop_rect_rotated_90() {
    let crop = CropRect::new(0.1, 0.2, 0.3, 0.4);
    let r = crop.rotated(90);
    assert!(approx(r.x, 1.0 - 0.2 - 0.4));
    assert!(approx(r.y, 0.1));
    assert!(approx(r.width, 0.4));
    assert!(approx(r.height, 0.3));
}

#[test]
fn crop_rect_rotated_180() {
    let crop = CropRect::new(0.1, 0.2, 0.3, 0.4);
    let r = crop.rotated(180);
    assert!(approx(r.x, 1.0 - 0.1 - 0.3));
    assert!(approx(r.y, 1.0 - 0.2 - 0.4));
    assert!(approx(r.width, 0.3));
    assert!(approx(r.height, 0.4));
}

#[test]
fn crop_rect_rotated_270_and_negative_90() {
    let crop = CropRect::new(0.1, 0.2, 0.3, 0.4);
    let neg90 = crop.rotated(-90);
    let pos270 = crop.rotated(270);
    assert_eq!(neg90, pos270);
    assert!(approx(pos270.x, 0.2));
    assert!(approx(pos270.y, 1.0 - 0.1 - 0.3));
    assert!(approx(pos270.width, 0.4));
    assert!(approx(pos270.height, 0.3));
}

#[test]
fn crop_rect_four_rotations_returns_original() {
    let crop = CropRect::new(0.1, 0.2, 0.3, 0.4);
    let mut r = crop;
    for _ in 0..4 {
        r = r.rotated(90);
    }
    assert!(approx(r.x, crop.x));
    assert!(approx(r.y, crop.y));
    assert!(approx(r.width, crop.width));
    assert!(approx(r.height, crop.height));
}

#[test]
fn crop_rect_flipped_h_and_double_flip() {
    let crop = CropRect::new(0.1, 0.2, 0.3, 0.4);
    let r = crop.flipped_h();
    assert!(approx(r.x, 1.0 - 0.1 - 0.3));
    assert!(approx(r.y, 0.2));
    let back = r.flipped_h();
    assert!(approx(back.x, crop.x));
    assert!(approx(back.y, crop.y));
}

#[test]
fn crop_rect_pixel_rect() {
    let crop = CropRect::new(0.25, 0.5, 0.5, 0.25);
    let rect = crop.pixel_rect(1920, 1080);
    assert!(approx(rect.x, 480.0));
    assert!(approx(rect.y, 540.0));
    assert!(approx(rect.width, 960.0));
    assert!(approx(rect.height, 270.0));
}

#[test]
fn composite_edit_basic() {
    let c = CompositeEdit::from(&[]);
    assert_eq!(c.rotation, 0);
    assert!(!c.flip_h);
    assert!(c.crop.is_none());

    let c = CompositeEdit::from(&[EditOp::Rotate(90)]);
    assert_eq!(c.rotation, 90);

    let c = CompositeEdit::from(&[EditOp::Rotate(-90)]);
    assert_eq!(c.rotation, 270);

    let c = CompositeEdit::from(&[EditOp::Rotate(90), EditOp::Rotate(90)]);
    assert_eq!(c.rotation, 180);

    let ops = vec![EditOp::Rotate(90); 4];
    let c = CompositeEdit::from(&ops);
    assert_eq!(c.rotation, 0);
}

#[test]
fn composite_edit_crop_replaces() {
    let c1 = CropRect::new(0.1, 0.1, 0.5, 0.5);
    let c2 = CropRect::new(0.2, 0.2, 0.3, 0.3);
    let c = CompositeEdit::from(&[EditOp::Crop(c1), EditOp::Crop(c2)]);
    assert_eq!(c.crop, Some(c2));
    assert_eq!(c.rotation, 0);
}

#[test]
fn composite_edit_flip() {
    let c = CompositeEdit::from(&[EditOp::FlipH]);
    assert!(c.flip_h);
    assert_eq!(c.rotation, 0);

    let c = CompositeEdit::from(&[EditOp::FlipH, EditOp::FlipH]);
    assert!(!c.flip_h);
    assert_eq!(c.rotation, 0);
}

#[test]
fn composite_edit_crop_then_rotate_transforms_crop() {
    let crop = CropRect::new(0.1, 0.2, 0.3, 0.4);
    let c = CompositeEdit::from(&[EditOp::Crop(crop), EditOp::Rotate(90)]);
    assert_eq!(c.rotation, 90);
    let expected = crop.rotated(90);
    let got = c.crop.unwrap();
    assert!(approx(got.x, expected.x));
    assert!(approx(got.y, expected.y));
    assert!(approx(got.width, expected.width));
    assert!(approx(got.height, expected.height));
}

#[test]
fn composite_edit_crop_then_flip_transforms_crop() {
    let crop = CropRect::new(0.1, 0.2, 0.3, 0.4);
    let c = CompositeEdit::from(&[EditOp::Crop(crop), EditOp::FlipH]);
    assert!(c.flip_h);
    let expected = crop.flipped_h();
    let got = c.crop.unwrap();
    assert!(approx(got.x, expected.x));
    assert!(approx(got.y, expected.y));
}
