use spectrum_core::cover::{cover_label, first_cover, list_subfolders};
use spectrum_core::scan::MediaKind;
use spectrum_testkit::fixture;
use std::path::PathBuf;

fn dir(tag: &str) -> PathBuf {
    let d = PathBuf::from(format!("/tmp/spectrum-cover-{tag}"));
    std::fs::create_dir_all(&d).unwrap();
    for e in std::fs::read_dir(&d).unwrap() {
        let p = e.unwrap().path();
        if p.is_file() {
            std::fs::remove_file(&p).unwrap();
        }
    }
    d
}

#[test]
fn image_beats_media_even_with_a_larger_name() {
    let d = dir("image-wins");
    fixture::write_fixture(&d, "z.heic", 9, 18, 0, true);
    fixture::write_mp4_fixture(&d, "a.mp4", 9, 16, 0, true);
    let (cover, _date, kind) = first_cover(&d).expect("cover");
    assert_eq!(cover.file_name().unwrap().to_string_lossy(), "z.heic");
    assert_eq!(kind, MediaKind::Photo);
}

#[test]
fn media_is_the_fallback_when_there_is_no_image() {
    let d = dir("media-fallback");
    fixture::write_mp4_fixture(&d, "b.mp4", 9, 16, 0, true);
    let (cover, _date, kind) = first_cover(&d).expect("cover");
    assert_eq!(cover.file_name().unwrap().to_string_lossy(), "b.mp4");
    assert_eq!(kind, MediaKind::Video);
}

#[test]
fn hidden_files_are_skipped() {
    let d = dir("hidden");
    fixture::write_png_fixture(&d, ".a.png", 4, 4, [255, 0, 0]);
    fixture::write_png_fixture(&d, "b.png", 4, 4, [0, 255, 0]);
    let (cover, _date, _) = first_cover(&d).expect("cover");
    assert_eq!(cover.file_name().unwrap().to_string_lossy(), "b.png");
}

#[test]
fn smallest_name_wins_within_the_same_group() {
    let d = dir("smallest");
    fixture::write_png_fixture(&d, "b.png", 4, 4, [0, 0, 255]);
    fixture::write_png_fixture(&d, "a.png", 4, 4, [0, 0, 255]);
    fixture::write_mp4_fixture(&d, "0.mp4", 9, 16, 0, true);
    let (cover, _date, _) = first_cover(&d).expect("cover");
    assert_eq!(cover.file_name().unwrap().to_string_lossy(), "a.png");
}

#[test]
fn cover_lookup_is_not_recursive() {
    let d = dir("non-recursive");
    let sub = d.join("nested");
    std::fs::create_dir_all(&sub).unwrap();
    fixture::write_png_fixture(&sub, "a.png", 4, 4, [255, 0, 0]);
    fixture::write_png_fixture(&d, "z.png", 4, 4, [0, 255, 0]);
    let (cover, _date, _) = first_cover(&d).expect("cover");
    assert_eq!(cover.file_name().unwrap().to_string_lossy(), "z.png");
}

#[test]
fn subfolders_are_sorted_and_carry_cover_and_date() {
    let d = dir("subfolders");
    let b = d.join("b-folder");
    let a = d.join("a-folder");
    std::fs::create_dir_all(&b).unwrap();
    std::fs::create_dir_all(&a).unwrap();
    fixture::write_png_fixture(&b, "z.png", 4, 4, [255, 0, 0]);
    // a-folder has no media at all
    let infos = list_subfolders(&d);
    let names: Vec<&str> = infos.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, vec!["a-folder", "b-folder"]);
    assert_eq!(infos[0].cover, None);
    assert_eq!(
        infos[1]
            .cover
            .as_ref()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy(),
        "z.png"
    );
    assert!(
        infos[1].cover_date.is_some(),
        "cover_date must come from mtime"
    );
    assert_eq!(infos[1].cover_kind, Some(MediaKind::Photo));
}

#[test]
fn cover_label_is_the_accessible_row_text() {
    let d = dir("label");
    let sub = d.join("photos");
    std::fs::create_dir_all(&sub).unwrap();
    fixture::write_png_fixture(&sub, "a.png", 4, 4, [0, 0, 255]);
    let empty = d.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let infos = list_subfolders(&d);
    assert_eq!(cover_label(&infos[0]), "empty  ·  (no cover)");
    assert_eq!(cover_label(&infos[1]), "photos  ·  a.png");
}
