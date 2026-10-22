use std::time::{Duration, SystemTime};

use spectrum_core::status_bar::StatusBarModel;

fn now() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000)
}

#[test]
fn hidden_until_a_task_or_message_exists() {
    let bar = StatusBarModel::new();
    assert!(!bar.is_visible());
    assert_eq!(bar.text(), "");

    let mut bar = StatusBarModel::new();
    bar.set_global("prefetching folder tree");
    assert!(bar.is_visible());
    assert_eq!(bar.text(), "prefetching folder tree");
}

#[test]
fn indeterminate_task_shows_only_the_label() {
    let mut bar = StatusBarModel::new();
    bar.begin("scanning");
    assert!(bar.active);
    assert_eq!(bar.total, None);
    assert_eq!(bar.text(), "scanning");
}

#[test]
fn determinate_task_shows_progress_and_clamps_to_total() {
    let mut bar = StatusBarModel::new();
    bar.begin_total("copying", 10);
    bar.update(3, "copying a.png");
    assert_eq!(bar.text(), "copying a.png 3/10");

    // a stray update cannot show "12/10"
    bar.update(12, "copying");
    assert_eq!(bar.done, 10);
    assert_eq!(bar.text(), "copying 10/10");
}

#[test]
fn completion_message_expires_at_the_deadline() {
    let mut bar = StatusBarModel::with_ttl(Duration::from_secs(10));
    bar.begin("scanning");
    bar.finish("scan complete", now());
    assert!(!bar.active);
    assert_eq!(bar.text(), "scan complete");

    // still visible 9 s later, gone at exactly 10 s
    assert!(!bar.tick(now() + Duration::from_secs(9)));
    assert_eq!(bar.text(), "scan complete");
    assert!(bar.is_visible());

    assert!(bar.tick(now() + Duration::from_secs(10)));
    assert!(!bar.is_visible());

    // ticking again changes nothing
    assert!(!bar.tick(now() + Duration::from_secs(11)));
}

#[test]
fn starting_a_task_cancels_the_pending_completion_timer() {
    let mut bar = StatusBarModel::new();
    bar.begin("scanning");
    bar.finish("scan complete", now());

    bar.begin("copying");
    assert!(bar.active);
    assert_eq!(bar.done_message, None);

    // the old deadline must not clear the new task's label
    bar.tick(now() + Duration::from_secs(30));
    assert!(bar.is_visible());
    assert_eq!(bar.text(), "copying");
}

#[test]
fn background_label_persists_until_finish_global() {
    let mut bar = StatusBarModel::new();
    bar.set_global("prefetching");
    // no deadline: a plain background label never expires on its own
    assert!(!bar.tick(now() + Duration::from_secs(600)));
    assert_eq!(bar.text(), "prefetching");

    bar.finish_global("prefetch done", now());
    assert_eq!(bar.text(), "prefetch done");
    assert!(bar.tick(now() + Duration::from_secs(10)));
    assert!(!bar.is_visible());
}

#[test]
fn active_task_wins_over_messages() {
    let mut bar = StatusBarModel::new();
    bar.set_global("prefetching");
    bar.begin_total("scanning", 5);
    bar.update(1, "scanning");
    assert_eq!(bar.text(), "scanning 1/5");

    bar.finish("scan complete", now());
    assert_eq!(bar.text(), "scan complete");

    // a background label is the fallback only when nothing else is present
    bar.done_message = None;
    assert_eq!(bar.text(), "prefetching");
}
