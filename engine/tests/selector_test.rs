use dj_engine::library::LoadedTrack;
use dj_engine::recency::RecencyRing;
use dj_engine::selector::Selector;
use dj_engine::sidecar::Sidecar;

fn track(path: &str, bpm: f64, key: &str, energy: f64, mixable: bool) -> LoadedTrack {
    let sc = Sidecar {
        schema_version: 1,
        analyzer_version: "0.1.0".into(),
        source_path: path.into(),
        source_mtime: 0.0,
        source_size_bytes: 0,
        error: None,
        duration_seconds: Some(200.0),
        sample_rate: Some(44100),
        bpm: Some(bpm),
        bpm_confidence: Some(0.9),
        beats_seconds: vec![0.0],
        downbeats_seconds: vec![0.0],
        key: Some(key.into()),
        key_label: Some("".into()),
        key_confidence: Some(0.8),
        energy_curve: vec![energy],
        energy_mean: Some(energy),
        cue_in_seconds: Some(0.0),
        cue_out_seconds: Some(195.0),
        intro_end_seconds: Some(16.0),
        outro_start_seconds: Some(180.0),
        is_mixable: Some(mixable),
        mixable_reasons: vec![],
        phrase_boundaries_seconds: None,
        phrase_confidence: None,
        band_energy_curves: None,
        band_energy_means: None,
        is_beatmatchable: None,
        is_phrase_aligned: None,
    };
    LoadedTrack {
        sidecar_path: path.into(),
        sidecar: sc,
    }
}

#[test]
fn picks_compatible_track() {
    let library = vec![
        track("a", 120.0, "8A", 0.5, true),
        track("b", 123.0, "8A", 0.5, true), // identical key, close BPM
        track("c", 145.0, "3A", 0.9, true), // far away
    ];
    let recency = RecencyRing::new(20);
    let mut sel = Selector::new(&library, &recency, 0.5);
    let current = &library[0];
    let next = sel.pick_next(current).expect("must pick");
    assert_eq!(next.sidecar.source_path, "b");
}

#[test]
fn falls_back_when_no_strict_match() {
    let library = vec![
        track("a", 120.0, "8A", 0.5, true),
        track("z", 150.0, "3B", 0.9, true), // way off but only candidate
    ];
    let recency = RecencyRing::new(20);
    let mut sel = Selector::new(&library, &recency, 0.5);
    let current = &library[0];
    let next = sel.pick_next(current).expect("fallback should still pick");
    assert_eq!(next.sidecar.source_path, "z");
}

#[test]
fn skips_recently_played() {
    let library = vec![
        track("a", 120.0, "8A", 0.5, true),
        track("b", 121.0, "8A", 0.5, true),
        track("c", 121.0, "8A", 0.5, true),
    ];
    let mut recency = RecencyRing::new(20);
    recency.record("b".into());
    let mut sel = Selector::new(&library, &recency, 0.5);
    let current = &library[0];
    let next = sel.pick_next(current).unwrap();
    assert_eq!(next.sidecar.source_path, "c");
}
