use dj_engine::phrase::phrase_boundaries;
use dj_engine::sidecar::Sidecar;

fn make_sc(downbeats: Vec<f64>, intro_end: f64, outro_start: f64) -> Sidecar {
    Sidecar {
        schema_version: 1,
        analyzer_version: "0.1.0".into(),
        source_path: "x".into(),
        source_mtime: 0.0,
        source_size_bytes: 0,
        error: None,
        duration_seconds: Some(200.0),
        sample_rate: Some(44100),
        bpm: Some(120.0),
        bpm_confidence: Some(0.9),
        beats_seconds: vec![],
        downbeats_seconds: downbeats,
        key: Some("8A".into()),
        key_label: Some("A minor".into()),
        key_confidence: Some(0.8),
        energy_curve: vec![],
        energy_mean: Some(0.4),
        cue_in_seconds: Some(0.0),
        cue_out_seconds: Some(200.0),
        intro_end_seconds: Some(intro_end),
        outro_start_seconds: Some(outro_start),
        is_mixable: Some(true),
        mixable_reasons: vec![],
    }
}

#[test]
fn anchors_on_first_downbeat_at_or_after_intro_end() {
    // Downbeats every 2s from 0 to 60 (i.e., 31 downbeats).
    let downbeats: Vec<f64> = (0..31).map(|i| i as f64 * 2.0).collect();
    let sc = make_sc(downbeats, 9.0, 60.0);
    let bounds = phrase_boundaries(&sc, 16);
    // First downbeat at or after intro_end=9.0 is at 10.0 (index 5).
    // Phrase boundaries: index 5, 21 (15 + 5+16). Index 37 is past outro_start.
    // So we expect [10.0, 42.0] — but 42 is also before 60, and 42 + 16*2=74 is past, so just [10.0, 42.0].
    assert_eq!(bounds, vec![10.0, 42.0]);
}

#[test]
fn spacing_is_phrase_bars_downbeats_apart() {
    let downbeats: Vec<f64> = (0..50).map(|i| i as f64).collect(); // 1s apart
    let sc = make_sc(downbeats, 0.0, 50.0);
    let bounds = phrase_boundaries(&sc, 8);
    // First boundary at downbeat[0] = 0.0. Then every 8th downbeat: 0, 8, 16, 24, 32, 40, 48.
    assert_eq!(bounds, vec![0.0, 8.0, 16.0, 24.0, 32.0, 40.0, 48.0]);
}

#[test]
fn stops_before_outro_start() {
    let downbeats: Vec<f64> = (0..100).map(|i| i as f64).collect();
    let sc = make_sc(downbeats, 0.0, 20.0);
    let bounds = phrase_boundaries(&sc, 16);
    // All boundaries must be < 20.
    assert!(bounds.iter().all(|b| *b < 20.0), "got {:?}", bounds);
}

#[test]
fn short_body_returns_empty_or_one() {
    let downbeats = vec![0.0, 1.0, 2.0, 3.0]; // only 4 downbeats
    let sc = make_sc(downbeats, 0.0, 4.0);
    let bounds = phrase_boundaries(&sc, 16);
    // Asking for 16-bar phrases but only 4 downbeats → just the first if present, else empty.
    // First downbeat at intro_end=0 IS index 0 = 0.0; next would be at index 16 (doesn't exist).
    // So just [0.0].
    assert_eq!(bounds, vec![0.0]);
}

#[test]
fn empty_downbeats_returns_empty() {
    let sc = make_sc(vec![], 0.0, 60.0);
    let bounds = phrase_boundaries(&sc, 16);
    assert!(bounds.is_empty());
}

#[test]
fn missing_intro_end_falls_back_to_zero() {
    let mut sc = make_sc(vec![0.0, 1.0, 2.0, 3.0], 0.0, 60.0);
    sc.intro_end_seconds = None;
    let bounds = phrase_boundaries(&sc, 4);
    // Anchor at 0, every 4th downbeat: 0.0 (and that's all within the 4 downbeats).
    assert_eq!(bounds, vec![0.0]);
}
