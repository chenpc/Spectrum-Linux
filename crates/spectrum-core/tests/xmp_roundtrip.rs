use std::path::PathBuf;

use spectrum_core::{xmp, CompositeEdit, CropRect, EditOp};

fn temp_path(name: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("spectrum-xmp-test-{}-{}", std::process::id(), name));
    p
}

#[test]
fn sidecar_path_preserves_original_extension() {
    assert_eq!(
        xmp::sidecar_path_for(PathBuf::from("photo.heic").as_path()),
        PathBuf::from("photo.heic.xmp")
    );
    assert_eq!(
        xmp::sidecar_path_for(PathBuf::from("clip.mp4").as_path()),
        PathBuf::from("clip.mp4.xmp")
    );
}

#[test]
fn round_trip_all_orientations() {
    for ori in 1..=8 {
        let ops = [
            EditOp::Rotate(90),
            EditOp::FlipH,
            EditOp::Crop(CropRect::new(0.1, 0.2, 0.3, 0.4)),
        ];
        let edit = CompositeEdit::from(&ops);
        let path = temp_path(&format!("ori{ori}.heic"));

        let written = xmp::write_sidecar(&path, &edit, ori, None).unwrap();
        let read = xmp::read_sidecar(&path, ori).unwrap();

        assert_eq!(read.rotation, edit.rotation, "orientation {ori}");
        assert_eq!(read.flip_h, edit.flip_h, "orientation {ori}");
        assert_eq!(read.crop, edit.crop, "orientation {ori}");

        let _ = std::fs::remove_file(written);
    }
}

#[test]
fn gyro_config_round_trip_with_xml_special_chars() {
    let edit = CompositeEdit::from(&[EditOp::Rotate(-90)]);
    let gyro = r#"{"smoothing":{"pitch":0.5},"note":"a & b < c"}"#;
    let path = temp_path("gyro.heic");

    let written = xmp::write_sidecar(&path, &edit, 6, Some(gyro)).unwrap();
    let read = xmp::read_sidecar(&path, 6).unwrap();

    assert_eq!(read.gyro_config.as_deref(), Some(gyro));
    assert_eq!(read.rotation, 270);

    let _ = std::fs::remove_file(written);
}

#[test]
fn identity_edit_returns_none() {
    let edit = CompositeEdit::from(&[]);
    let path = temp_path("identity.heic");
    xmp::write_sidecar(&path, &edit, 1, None).unwrap();
    assert!(xmp::read_sidecar(&path, 1).is_none());
    xmp::delete_sidecar(&path);
    assert!(xmp::read_sidecar(&path, 1).is_none());
}

#[test]
fn missing_sidecar_returns_none() {
    assert!(xmp::read_sidecar(PathBuf::from("does-not-exist.heic").as_path(), 1).is_none());
}
