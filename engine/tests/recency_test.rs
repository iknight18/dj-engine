use dj_engine::recency::RecencyRing;

#[test]
fn rejects_recently_played() {
    let mut r = RecencyRing::new(3);
    r.record("a".into());
    r.record("b".into());
    assert!(r.was_recent("a"));
    assert!(r.was_recent("b"));
    assert!(!r.was_recent("c"));
}

#[test]
fn rolls_over_capacity() {
    let mut r = RecencyRing::new(2);
    r.record("a".into());
    r.record("b".into());
    r.record("c".into());
    // "a" was evicted.
    assert!(!r.was_recent("a"));
    assert!(r.was_recent("b"));
    assert!(r.was_recent("c"));
}

#[test]
fn zero_capacity_records_nothing() {
    let mut r = RecencyRing::new(0);
    r.record("a".into());
    assert!(!r.was_recent("a"));
}
