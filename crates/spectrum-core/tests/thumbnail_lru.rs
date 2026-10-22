//! LRU eviction for the thumbnail cache.

use spectrum_core::thumbnail::ThumbnailService;
use spectrum_testkit::fixture;

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("spectrum-core-{tag}"));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn evicts_the_least_recently_used_entry() {
    let dir = temp_dir("lru_evict");
    let a = fixture::write_png_fixture(&dir, "a.png", 100, 100, [1, 2, 3]);
    let b = fixture::write_png_fixture(&dir, "b.png", 100, 100, [4, 5, 6]);
    let c = fixture::write_png_fixture(&dir, "c.png", 100, 100, [7, 8, 9]);

    let mut svc = ThumbnailService::with_capacity(64, 2);
    svc.get(&a).unwrap();
    svc.get(&b).unwrap();
    assert_eq!(svc.len(), 2);
    assert_eq!(svc.evictions(), 0);

    svc.get(&c).unwrap();
    assert_eq!(svc.len(), 2, "cache must not exceed capacity");
    assert_eq!(svc.evictions(), 1);
    assert!(!svc.is_cached(&a), "a was the least recently used");
    assert!(svc.is_cached(&b));
    assert!(svc.is_cached(&c));
}

#[test]
fn a_cache_hit_refreshes_the_order() {
    let dir = temp_dir("lru_touch");
    let a = fixture::write_png_fixture(&dir, "a.png", 100, 100, [1, 2, 3]);
    let b = fixture::write_png_fixture(&dir, "b.png", 100, 100, [4, 5, 6]);
    let c = fixture::write_png_fixture(&dir, "c.png", 100, 100, [7, 8, 9]);

    let mut svc = ThumbnailService::with_capacity(2, 2);
    svc.get(&a).unwrap();
    svc.get(&b).unwrap();

    // Touching `a` makes `b` the oldest, so the next insert must evict `b`, not `a`.
    let (_, t) = svc.get(&a).unwrap();
    assert!(t.cached);
    svc.get(&c).unwrap();
    assert!(
        svc.is_cached(&a),
        "a was used most recently and must survive"
    );
    assert!(!svc.is_cached(&b));
    assert!(svc.is_cached(&c));
    assert_eq!(svc.evictions(), 1);
    // the evicted file decodes again on the next request
    let (_, t) = svc.get(&b).unwrap();
    assert!(!t.cached);
    assert_eq!(svc.decodes(), 4);
}

#[test]
fn unbounded_service_never_evicts() {
    let dir = temp_dir("lru_unbounded");
    let mut svc = ThumbnailService::new(64);
    for i in 0..5 {
        let p = fixture::write_png_fixture(&dir, &format!("{i}.png", i = i), 20, 20, [0, 0, 0]);
        svc.get(&p).unwrap();
    }
    assert_eq!(svc.len(), 5);
    assert_eq!(svc.evictions(), 0);
    assert_eq!(svc.capacity(), usize::MAX);
}
