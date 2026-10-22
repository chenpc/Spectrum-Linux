use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use spectrum_core::import::{date_key, ImportModel, ImportRow};
use spectrum_testkit::fixture;

fn at(secs: u64) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
}

fn fresh(tag: &str) -> PathBuf {
    let dir = PathBuf::from(format!("/tmp/spectrum-import-tests/{tag}"));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn date_key_is_the_utc_calendar_day() {
    assert_eq!(date_key(at(1_700_000_000)), "2023-11-14");
    // last second of the previous UTC day: 19674 days -> 2023-11-13
    assert_eq!(date_key(at(19674 * 86400 + 86399)), "2023-11-13");
}

#[test]
fn groups_are_newest_day_first() {
    let dir = fresh("newest_first");
    // 1700000000 -> 2023-11-14, 1684400000 -> 2023-05-18, 1691000000 -> 2023-08-02
    fixture::write_png_fixture(&dir, "a.png", 4, 4, [1, 2, 3]);
    std::fs::File::options()
        .write(true)
        .open(dir.join("a.png"))
        .unwrap()
        .set_modified(at(1_700_000_000))
        .unwrap();
    fixture::write_png_fixture(&dir, "b.png", 4, 4, [1, 2, 3]);
    std::fs::File::options()
        .write(true)
        .open(dir.join("b.png"))
        .unwrap()
        .set_modified(at(1_684_400_000))
        .unwrap();
    fixture::write_png_fixture(&dir, "c.png", 4, 4, [1, 2, 3]);
    std::fs::File::options()
        .write(true)
        .open(dir.join("c.png"))
        .unwrap()
        .set_modified(at(1_691_000_000))
        .unwrap();

    let model = ImportModel::scan(&dir);
    let keys: Vec<String> = model.groups.iter().map(|g| g.key.clone()).collect();
    assert_eq!(keys, vec!["2023-11-14", "2023-08-02", "2023-05-18"]);
    assert_eq!(model.group_label(0), "2023-11-14  ·  1");
}

#[test]
fn items_inside_a_group_are_name_ascending() {
    let dir = fresh("name_order");
    for name in ["z.png", "a.png", "m.png"] {
        fixture::write_png_fixture(&dir, name, 4, 4, [1, 2, 3]);
        std::fs::File::options()
            .write(true)
            .open(dir.join(name))
            .unwrap()
            .set_modified(at(1_700_000_000))
            .unwrap();
    }
    let model = ImportModel::scan(&dir);
    assert_eq!(model.groups.len(), 1);
    let names: Vec<String> = model.groups[0]
        .indices
        .iter()
        .map(|&i| model.item_name(i))
        .collect();
    assert_eq!(names, vec!["a.png", "m.png", "z.png"]);
}

#[test]
fn rows_follow_expansion() {
    let dir = fresh("expansion");
    fixture::write_png_fixture(&dir, "a.png", 4, 4, [1, 2, 3]);
    std::fs::File::options()
        .write(true)
        .open(dir.join("a.png"))
        .unwrap()
        .set_modified(at(1_700_000_000))
        .unwrap();
    fixture::write_png_fixture(&dir, "b.png", 4, 4, [1, 2, 3]);
    std::fs::File::options()
        .write(true)
        .open(dir.join("b.png"))
        .unwrap()
        .set_modified(at(1_684_400_000))
        .unwrap();

    let mut model = ImportModel::scan(&dir);
    assert_eq!(model.rows().len(), 4); // 2 headers + 2 items

    model.toggle(0);
    assert_eq!(
        model.rows(),
        vec![
            ImportRow::Header(0),
            ImportRow::Header(1),
            ImportRow::Item(1)
        ]
    );

    model.set_all_expanded(false);
    assert_eq!(
        model.rows(),
        vec![ImportRow::Header(0), ImportRow::Header(1)]
    );

    model.set_all_expanded(true);
    assert_eq!(model.rows().len(), 4);
}

#[test]
fn removing_moved_items_drops_empty_groups_and_keeps_expansion_by_key() {
    let dir = fresh("remove");
    fixture::write_png_fixture(&dir, "a.png", 4, 4, [1, 2, 3]);
    std::fs::File::options()
        .write(true)
        .open(dir.join("a.png"))
        .unwrap()
        .set_modified(at(1_700_000_000))
        .unwrap();
    fixture::write_png_fixture(&dir, "b.png", 4, 4, [1, 2, 3]);
    std::fs::File::options()
        .write(true)
        .open(dir.join("b.png"))
        .unwrap()
        .set_modified(at(1_684_400_000))
        .unwrap();
    fixture::write_png_fixture(&dir, "c.png", 4, 4, [1, 2, 3]);
    std::fs::File::options()
        .write(true)
        .open(dir.join("c.png"))
        .unwrap()
        .set_modified(at(1_684_400_000))
        .unwrap();

    let mut model = ImportModel::scan(&dir);
    // newest group collapsed, older group expanded
    model.toggle(0);
    assert_eq!(model.groups[0].key, "2023-11-14");
    assert!(!model.groups[0].expanded);
    assert!(model.groups[1].expanded);

    model.remove_items(&[dir.join("a.png")]);
    assert_eq!(model.groups.len(), 1);
    assert_eq!(model.groups[0].key, "2023-05-18");
    assert_eq!(model.group_label(0), "2023-05-18  ·  2");
    assert_eq!(
        model.rows(),
        vec![ImportRow::Header(0), ImportRow::Item(0), ImportRow::Item(1)]
    );
}

#[test]
fn non_media_files_are_skipped() {
    let dir = fresh("skip_non_media");
    fixture::write_png_fixture(&dir, "a.png", 4, 4, [1, 2, 3]);
    std::fs::write(dir.join("notes.txt"), "not media").unwrap();
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    let model = ImportModel::scan(&dir);
    assert_eq!(model.items.len(), 1);
    assert_eq!(model.groups.len(), 1);
}
