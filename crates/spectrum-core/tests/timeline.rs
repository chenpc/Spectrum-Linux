use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use spectrum_core::scan::{MediaEntry, MediaKind};
use spectrum_core::timeline::{group_by_month, group_label, month_key, UNKNOWN_GROUP};

fn entry(name: &str, secs: u64) -> MediaEntry {
    MediaEntry {
        path: PathBuf::from(name),
        kind: MediaKind::Photo,
        signal: None,
        edit: None,
        video: None,
        mtime: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs)),
        size: 0,
    }
}

#[test]
fn month_key_uses_utc_calendar() {
    // Values are UTC epoch seconds for the first day of each month.
    assert_eq!(month_key(SystemTime::UNIX_EPOCH), "1970-01");
    assert_eq!(
        month_key(SystemTime::UNIX_EPOCH + Duration::from_secs(1735689600)),
        "2025-01"
    );
    assert_eq!(
        month_key(SystemTime::UNIX_EPOCH + Duration::from_secs(1780272000)),
        "2026-06"
    );
    assert_eq!(
        month_key(SystemTime::UNIX_EPOCH + Duration::from_secs(1791158400)),
        "2026-10"
    );
    // Last day of the month stays in the same group.
    assert_eq!(
        month_key(SystemTime::UNIX_EPOCH + Duration::from_secs(1793404800)),
        "2026-10"
    );
    // A pre-epoch time has no calendar month.
    assert_eq!(
        month_key(SystemTime::UNIX_EPOCH - Duration::from_secs(86400)),
        UNKNOWN_GROUP
    );
}

#[test]
fn groups_are_newest_first() {
    let entries = vec![
        entry("old.png", 1735689600), // 2025-01
        entry("mid.png", 1780272000), // 2026-06
        entry("new.png", 1791158400), // 2026-10
    ];
    let groups = group_by_month(&entries, &[0, 1, 2]);
    assert_eq!(
        groups.iter().map(|g| g.key.clone()).collect::<Vec<_>>(),
        vec!["2026-10", "2026-06", "2025-01"]
    );
}

#[test]
fn order_within_a_group_follows_the_display_order() {
    let entries = vec![
        entry("a.png", 1791158400), // 2026-10
        entry("b.png", 1780272000), // 2026-06
        entry("c.png", 1791244800), // 2026-10
    ];
    // Display order is c, a, b (e.g. sorted newest-first), so the 2026-10 group keeps c then a.
    let groups = group_by_month(&entries, &[2, 0, 1]);
    assert_eq!(groups[0].key, "2026-10");
    assert_eq!(groups[0].indices, vec![2, 0]);
    assert_eq!(groups[1].key, "2026-06");
    assert_eq!(groups[1].indices, vec![1]);
}

#[test]
fn unknown_mtime_group_sorts_last() {
    let mut no_time = entry("undated.png", 0);
    no_time.mtime = None;
    let entries = vec![
        entry("new.png", 1791158400),
        no_time,
        entry("old.png", 1735689600),
    ];
    let groups = group_by_month(&entries, &[0, 1, 2]);
    assert_eq!(
        groups.iter().map(|g| g.key.clone()).collect::<Vec<_>>(),
        vec!["2026-10", "2025-01", UNKNOWN_GROUP]
    );
    assert_eq!(groups[2].indices, vec![1]);
}

#[test]
fn group_indices_are_entry_indices_not_position_in_order() {
    // A filtered order (only entries 1 and 3) must report the original entry indices.
    let entries = vec![
        entry("a.png", 1791158400),
        entry("b.png", 1780272000),
        entry("c.png", 1791158400),
        entry("d.png", 1780272000),
    ];
    let groups = group_by_month(&entries, &[3, 1]);
    assert_eq!(groups[0].key, "2026-06");
    assert_eq!(groups[0].indices, vec![3, 1]);
    assert_eq!(group_label(&groups[0]), "2026-06  ·  2");
}

#[test]
fn empty_input_has_no_groups() {
    assert_eq!(group_by_month(&[], &[]), vec![]);
}
