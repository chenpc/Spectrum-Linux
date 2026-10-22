use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use spectrum_core::cover::{
    list_subfolders, sort_by_cover, subfolders_are_in_order, SubfolderInfo,
};
use spectrum_testkit::fixture;

fn info(name: &str, date: Option<SystemTime>) -> SubfolderInfo {
    SubfolderInfo {
        name: name.to_string(),
        path: PathBuf::from(format!("/tmp/spectrum-cover-sort/{name}")),
        cover: date.map(|_| PathBuf::from(format!("/tmp/spectrum-cover-sort/{name}/cover.png"))),
        cover_date: date,
        cover_kind: date.map(|_| spectrum_core::scan::MediaKind::Photo),
    }
}

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

fn names(infos: &[SubfolderInfo]) -> Vec<String> {
    infos.iter().map(|i| i.name.clone()).collect()
}

#[test]
fn newest_cover_date_sorts_first() {
    let old = at(1780272000); // 2026-06-01
    let new = at(1791158400); // 2026-10-05
    let input = [info("a-old", Some(old)), info("b-new", Some(new))];
    assert_eq!(names(&sort_by_cover(&input)), vec!["b-new", "a-old"]);
    assert!(subfolders_are_in_order(&input[1], &input[0]));
}

#[test]
fn same_date_falls_back_to_name_ascending() {
    let d = at(1780272000);
    let input = [info("c-tie", Some(d)), info("a-tie", Some(d))];
    assert_eq!(names(&sort_by_cover(&input)), vec!["a-tie", "c-tie"]);
}

#[test]
fn folder_without_cover_is_last_even_when_its_name_sorts_first() {
    let d = at(1780272000);
    let input = [info("a-empty", None), info("b-with-cover", Some(d))];
    assert_eq!(
        names(&sort_by_cover(&input)),
        vec!["b-with-cover", "a-empty"]
    );
}

#[test]
fn all_without_cover_are_alphabetical() {
    let input = [info("c", None), info("a", None), info("b", None)];
    assert_eq!(names(&sort_by_cover(&input)), vec!["a", "b", "c"]);
}

#[test]
fn sort_is_a_permutation_of_the_input() {
    let d = at(1780272000);
    let input = [
        info("z", Some(d)),
        info("y", None),
        info("x", Some(at(1791158400))),
        info("w", None),
    ];
    let out = sort_by_cover(&input);
    assert_eq!(out.len(), input.len());
    let mut got: Vec<String> = names(&out);
    got.sort();
    let mut want: Vec<String> = names(&input);
    want.sort();
    assert_eq!(got, want);
}

#[test]
fn real_directories_are_ordered_by_cover_date() {
    let root = PathBuf::from("/tmp/spectrum-cover-sort-dirs");
    std::fs::create_dir_all(&root).unwrap();
    for sub in ["z-newest", "m-middle", "a-oldest", "b-tie", "c-empty"] {
        std::fs::create_dir_all(root.join(sub)).unwrap();
    }
    fixture::write_png_fixture(&root.join("z-newest"), "cover.png", 4, 4, [255, 0, 0]);
    fixture::write_png_fixture(&root.join("m-middle"), "cover.png", 4, 4, [255, 0, 0]);
    fixture::write_png_fixture(&root.join("a-oldest"), "cover.png", 4, 4, [255, 0, 0]);
    fixture::write_png_fixture(&root.join("b-tie"), "cover.png", 4, 4, [255, 0, 0]);

    set_mtime(&root.join("z-newest/cover.png"), 1791158400); // 2026-10-05
    set_mtime(&root.join("m-middle/cover.png"), 1785484800); // 2026-07-31
    set_mtime(&root.join("a-oldest/cover.png"), 1780272000); // 2026-06-01
    set_mtime(&root.join("b-tie/cover.png"), 1780272000); // same date as a-oldest

    let raw = list_subfolders(&root);
    assert_eq!(
        names(&raw),
        vec!["a-oldest", "b-tie", "c-empty", "m-middle", "z-newest"]
    );
    assert_eq!(
        names(&sort_by_cover(&raw)),
        vec!["z-newest", "m-middle", "a-oldest", "b-tie", "c-empty"]
    );
}

fn set_mtime(path: &PathBuf, secs: u64) {
    let t = SystemTime::UNIX_EPOCH + Duration::from_secs(secs);
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(t)
        .unwrap();
}
