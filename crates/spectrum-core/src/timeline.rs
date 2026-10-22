//! Timeline grouping: photos are grouped by month and the groups are shown newest-first
//! (macOS `PhotoGridView` timeline behaviour).
//!
//! Month keys are computed in UTC so results are reproducible across machines/timezones.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::scan::MediaEntry;

/// Group key for entries whose mtime is unknown; always sorted last.
pub const UNKNOWN_GROUP: &str = "unknown";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonthGroup {
    pub key: String,
    /// Entry indices, in the order they appear in `order` (the display order).
    pub indices: Vec<usize>,
}

/// `days_from_epoch` -> `(year, month, day)` (Howard Hinnant's `civil_from_days`).
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z0 = z + 719468;
    let era = if z0 >= 0 { z0 } else { z0 - 146096 } / 146097;
    let doe = (z0 - era * 146097) as u64; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn month_key(t: SystemTime) -> String {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => {
            let (y, m, _) = civil_from_days(d.as_secs() as i64 / 86400);
            format!("{y:04}-{m:02}")
        }
        Err(_) => UNKNOWN_GROUP.to_string(),
    }
}

/// `order` is the display order (already filtered + sorted); groups are newest-first and keep the
/// order inside each group. Entries with unknown mtime land in the `unknown` group, sorted last.
pub fn group_by_month(entries: &[MediaEntry], order: &[usize]) -> Vec<MonthGroup> {
    let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
    for &i in order {
        let key = entries[i]
            .mtime
            .map(month_key)
            .unwrap_or_else(|| UNKNOWN_GROUP.to_string());
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some(g) => g.1.push(i),
            None => groups.push((key, vec![i])),
        }
    }
    // `unknown` must sort last even though it is lexicographically larger than a `YYYY-MM` key.
    groups.sort_by(|a, b| {
        (a.0 == UNKNOWN_GROUP)
            .cmp(&(b.0 == UNKNOWN_GROUP))
            .then(b.0.cmp(&a.0))
    });
    groups
        .into_iter()
        .map(|(key, indices)| MonthGroup { key, indices })
        .collect()
}

/// Row text used by the UI timeline list.
pub fn group_label(group: &MonthGroup) -> String {
    format!("{}  ·  {}", group.key, group.indices.len())
}
