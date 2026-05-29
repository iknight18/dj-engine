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
        phrase_boundaries_seconds: None,
        phrase_confidence: None,
        band_energy_curves: None,
        band_energy_means: None,
        is_beatmatchable: None,
        is_phrase_aligned: None,
        stems: None,
        local_bpm: vec![],
        beat_confidence: vec![],
        downbeat_confidence: vec![],
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

#[test]
fn uses_sidecar_phrase_boundaries_when_set() {
    let mut sc = make_sc(vec![0.0, 1.0, 2.0, 3.0], 0.0, 60.0);
    sc.phrase_boundaries_seconds = Some(vec![5.0, 10.0, 15.0]);
    sc.is_phrase_aligned = Some(true);
    let bounds = phrase_boundaries(&sc, 16);
    assert_eq!(bounds, vec![5.0, 10.0, 15.0]);
}

#[test]
fn empty_sidecar_phrase_boundaries_falls_back_to_heuristic() {
    let mut sc = make_sc(vec![0.0, 1.0, 2.0, 3.0], 0.0, 60.0);
    sc.phrase_boundaries_seconds = Some(vec![]);
    // The heuristic should kick in and produce [0.0] (anchor at intro_end=0).
    let bounds = phrase_boundaries(&sc, 4);
    assert_eq!(bounds, vec![0.0]);
}

#[test]
fn sidecar_boundaries_filtered_by_intro_end_and_outro_start() {
    // Simulates the 006-song.mp3 case: explicit phrase boundaries from the analyzer
    // start at 7.1s but intro_end is 32.4s — the engine should NOT crossfade into
    // the intro. Boundaries inside the intro must be filtered out.
    let mut sc = make_sc(vec![], 32.4, 154.7);
    sc.phrase_boundaries_seconds = Some(vec![
        7.1, 13.5, 19.9, 26.3, 32.7, 39.1, 45.5, 51.9, 58.3, 154.7, 160.0,
    ]);
    sc.is_phrase_aligned = Some(true);
    let bounds = phrase_boundaries(&sc, 16);
    // 7.1, 13.5, 19.9, 26.3 are before intro_end=32.4 → filtered out.
    // 154.7 is at outro_start → filtered out (strict <).
    // 160.0 is past outro_start → filtered out.
    // 32.7, 39.1, 45.5, 51.9, 58.3 → kept.
    assert_eq!(bounds, vec![32.7, 39.1, 45.5, 51.9, 58.3]);
}

#[test]
fn sidecar_boundaries_all_filtered_falls_back_to_heuristic() {
    // If every explicit boundary is inside the intro, the function should fall through
    // to the heuristic path rather than returning an empty list.
    let mut sc = make_sc(vec![40.0, 41.0, 42.0, 43.0], 40.0, 60.0);
    sc.phrase_boundaries_seconds = Some(vec![1.0, 2.0, 3.0]); // all pre-intro
    let bounds = phrase_boundaries(&sc, 4);
    // Heuristic kicks in: anchor at downbeat at/after intro_end=40 → index 0 (40.0),
    // step by 4 → only index 0 is in range.
    assert_eq!(bounds, vec![40.0]);
}
