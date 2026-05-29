use dj_engine::recipe::RecipeChoice;
use dj_engine::recipe_recency::RecipeRecency;

#[test]
fn new_ring_contains_nothing() {
    let r = RecipeRecency::new(5);
    assert!(!r.contains(RecipeChoice::EqualPower));
    assert!(!r.contains(RecipeChoice::HardCut));
}

#[test]
fn record_then_contains() {
    let mut r = RecipeRecency::new(5);
    r.record(RecipeChoice::RhythmSwap);
    assert!(r.contains(RecipeChoice::RhythmSwap));
    assert!(!r.contains(RecipeChoice::EqualPower));
}

#[test]
fn ring_evicts_oldest_at_capacity() {
    let mut r = RecipeRecency::new(2);
    r.record(RecipeChoice::EqualPower);
    r.record(RecipeChoice::RhythmSwap);
    assert!(r.contains(RecipeChoice::EqualPower));
    r.record(RecipeChoice::FilterFade);
    // EqualPower should be evicted now.
    assert!(!r.contains(RecipeChoice::EqualPower));
    assert!(r.contains(RecipeChoice::RhythmSwap));
    assert!(r.contains(RecipeChoice::FilterFade));
}

#[test]
fn capacity_zero_disables_recency() {
    let mut r = RecipeRecency::new(0);
    r.record(RecipeChoice::RhythmSwap);
    assert!(!r.contains(RecipeChoice::RhythmSwap));
}
