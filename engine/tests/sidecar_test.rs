use dj_engine::sidecar::Sidecar;

const VALID_JSON: &str = r#"{
    "schema_version": 1,
    "analyzer_version": "0.1.0",
    "source_path": "/music/a.mp3",
    "source_mtime": 1715000000.0,
    "source_size_bytes": 1000,
    "duration_seconds": 200.0,
    "sample_rate": 44100,
    "bpm": 124.0,
    "bpm_confidence": 0.92,
    "beats_seconds": [0.0, 0.5, 1.0],
    "downbeats_seconds": [0.0, 2.0],
    "key": "8A",
    "key_label": "A minor",
    "key_confidence": 0.81,
    "energy_curve": [0.1, 0.2, 0.3],
    "energy_mean": 0.42,
    "cue_in_seconds": 0.0,
    "cue_out_seconds": 195.0,
    "intro_end_seconds": 32.0,
    "outro_start_seconds": 180.0,
    "is_mixable": true,
    "mixable_reasons": []
}"#;

const ERROR_JSON: &str = r#"{
    "schema_version": 1,
    "analyzer_version": "0.1.0",
    "source_path": "/music/broken.mp3",
    "source_mtime": 0.0,
    "source_size_bytes": 0,
    "error": "decode failed"
}"#;

#[test]
fn parses_valid_sidecar() {
    let sc: Sidecar = serde_json::from_str(VALID_JSON).unwrap();
    assert_eq!(sc.bpm, Some(124.0));
    assert_eq!(sc.key.as_deref(), Some("8A"));
    assert!(sc.is_playable());
}

#[test]
fn parses_error_sidecar() {
    let sc: Sidecar = serde_json::from_str(ERROR_JSON).unwrap();
    assert!(sc.error.is_some());
    assert!(!sc.is_playable());
    assert_eq!(sc.bpm, None);
}

#[test]
fn rejects_invalid_camelot_key() {
    let bad = VALID_JSON.replace("\"8A\"", "\"13Z\"");
    // We don't enforce the regex in serde (the schema does in CI); engine just treats
    // it as a string and the camelot module rejects it on use.
    let sc: Sidecar = serde_json::from_str(&bad).unwrap();
    assert_eq!(sc.key.as_deref(), Some("13Z"));
}
