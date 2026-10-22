use std::path::Path;

use spectrum_core::scan::{kind_for, MediaKind};

#[test]
fn kind_for_recognises_photo_and_video_extensions() {
    assert_eq!(kind_for(Path::new("a.heic")), Some(MediaKind::Photo));
    assert_eq!(kind_for(Path::new("a.heif")), Some(MediaKind::Photo));
    assert_eq!(kind_for(Path::new("a.JPG")), Some(MediaKind::Photo));
    assert_eq!(kind_for(Path::new("a.png")), Some(MediaKind::Photo));
    assert_eq!(kind_for(Path::new("a.webp")), Some(MediaKind::Photo));
    assert_eq!(kind_for(Path::new("a.mp4")), Some(MediaKind::Video));
    assert_eq!(kind_for(Path::new("a.mov")), Some(MediaKind::Video));
    assert_eq!(kind_for(Path::new("a.txt")), None);
    assert_eq!(kind_for(Path::new("no_extension")), None);
}

#[test]
fn media_kind_label_is_stable() {
    assert_eq!(MediaKind::Photo.label(), "photo");
    assert_eq!(MediaKind::Video.label(), "video");
}
