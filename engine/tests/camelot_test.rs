use dj_engine::camelot::{CamelotKey, KeyMatch};

#[test]
fn parses_valid_keys() {
    assert!(CamelotKey::parse("8A").is_some());
    assert!(CamelotKey::parse("12B").is_some());
    assert!(CamelotKey::parse("1A").is_some());
}

#[test]
fn rejects_invalid_keys() {
    assert!(CamelotKey::parse("0A").is_none());
    assert!(CamelotKey::parse("13A").is_none());
    assert!(CamelotKey::parse("8C").is_none());
    assert!(CamelotKey::parse("XX").is_none());
}

#[test]
fn identical_key_is_match() {
    let a = CamelotKey::parse("8A").unwrap();
    assert_eq!(a.compatibility(&a), KeyMatch::Identical);
}

#[test]
fn adjacent_keys_are_match() {
    let a = CamelotKey::parse("8A").unwrap();
    let b = CamelotKey::parse("7A").unwrap();
    let c = CamelotKey::parse("9A").unwrap();
    assert_eq!(a.compatibility(&b), KeyMatch::Adjacent);
    assert_eq!(a.compatibility(&c), KeyMatch::Adjacent);
}

#[test]
fn wraps_around_12_to_1() {
    let a = CamelotKey::parse("12A").unwrap();
    let b = CamelotKey::parse("1A").unwrap();
    assert_eq!(a.compatibility(&b), KeyMatch::Adjacent);
}

#[test]
fn relative_major_minor_is_match() {
    let a = CamelotKey::parse("8A").unwrap();    // A minor
    let b = CamelotKey::parse("8B").unwrap();    // C major
    assert_eq!(a.compatibility(&b), KeyMatch::Relative);
}

#[test]
fn distant_keys_are_no_match() {
    let a = CamelotKey::parse("8A").unwrap();
    let b = CamelotKey::parse("3A").unwrap();
    assert_eq!(a.compatibility(&b), KeyMatch::None);
}
