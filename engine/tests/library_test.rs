use std::fs;
use std::path::PathBuf;

use dj_engine::library::load_library;

fn write_sidecar(dir: &std::path::Path, name: &str, body: &str) -> PathBuf {
    let p = dir.join(name);
    fs::write(&p, body).unwrap();
    p
}

const TEMPLATE: &str = r#"{
    "schema_version": 1,
    "analyzer_version": "0.1.0",
    "source_path": "__PATH__",
    "source_mtime": 0.0,
    "source_size_bytes": 0,
    "duration_seconds": 200.0,
    "sample_rate": 44100,
    "bpm": 120.0,
    "bpm_confidence": 0.9,
    "beats_seconds": [0.0],
    "downbeats_seconds": [0.0],
    "key": "8A",
    "key_label": "A minor",
    "key_confidence": 0.8,
    "energy_curve": [0.4],
    "energy_mean": 0.4,
    "cue_in_seconds": 0.0,
    "cue_out_seconds": 195.0,
    "intro_end_seconds": 16.0,
    "outro_start_seconds": 180.0,
    "is_mixable": true,
    "mixable_reasons": []
}"#;

#[test]
fn loads_valid_sidecars_drops_missing_audio_and_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let sc_dir = tmp.path().join("sidecars");
    fs::create_dir(&sc_dir).unwrap();
    let audio_dir = tmp.path().join("audio");
    fs::create_dir(&audio_dir).unwrap();

    let audio_a = audio_dir.join("a.mp3");
    fs::write(&audio_a, b"fake").unwrap();
    // Escape backslashes for JSON literal embedding (Windows paths use `\`).
    let audio_a_json = audio_a.to_string_lossy().replace('\\', "\\\\");
    let body_a = TEMPLATE.replace("__PATH__", &audio_a_json);
    write_sidecar(&sc_dir, "a.json", &body_a);

    // Sidecar with missing audio file — should be dropped.
    let body_missing = TEMPLATE.replace("__PATH__", "/nonexistent/missing.mp3");
    write_sidecar(&sc_dir, "missing.json", &body_missing);

    // Error sidecar — should be dropped.
    let err_body = r#"{
        "schema_version": 1,
        "analyzer_version": "0.1.0",
        "source_path": "/x.mp3",
        "source_mtime": 0.0,
        "source_size_bytes": 0,
        "error": "decode failed"
    }"#;
    write_sidecar(&sc_dir, "err.json", err_body);

    let tracks = load_library(&sc_dir).unwrap();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].sidecar.source_path, audio_a.to_string_lossy());
}

#[test]
fn errors_on_empty_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let result = load_library(tmp.path());
    assert!(result.is_err(), "expected EmptyLibrary, got {:?}", result);
}
