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
        stems: None,
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
    // The final fallback is now capped at 20% BPM deviation. A candidate at
    // 25% (150 vs 120 BPM) is beyond the cap and should not be picked; the
    // selector correctly returns None when no candidate is within range.
    let library = vec![
        track("a", 120.0, "8A", 0.5, true),
        track("z", 150.0, "3B", 0.9, true), // 25% BPM difference — beyond the 20% cap
    ];
    let recency = RecencyRing::new(20);
    let mut sel = Selector::new(&library, &recency, 0.5);
    let current = &library[0];
    assert!(
        sel.pick_next(current).is_none(),
        "selector should return None when only candidate is >20% away in BPM"
    );
}

#[test]
fn falls_back_within_bpm_cap() {
    // At exactly 19% BPM deviation (≤20%), the final fallback should still pick.
    let library = vec![
        track("a", 120.0, "8A", 0.5, true),
        track("z", 142.0, "3B", 0.9, true), // ~18% BPM difference — within 20% cap
    ];
    let recency = RecencyRing::new(20);
    let mut sel = Selector::new(&library, &recency, 0.5);
    let current = &library[0];
    let next = sel
        .pick_next(current)
        .expect("fallback should pick within 20% cap");
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

#[test]
fn prefers_smoother_transition_when_other_axes_tied() {
    // Two candidates that score identically on BPM/key/energy but differ wildly
    // in expected transition: one would produce a long blend, the other a hard cut.
    let library = vec![
        track("a", 120.0, "8A", 0.4, true),
        // "b": identical → long blend with current.
        track("b", 120.0, "8A", 0.4, true),
        // "c": same BPM/key/energy as b... but we force a huge energy gap into c by NOT setting
        // energy_mean — wait, that won't work; instead force is_mixable=false on c so transition
        // would be 16-bar fade (lower transition_score) instead of 32-bar long blend.
        track("c", 120.0, "8A", 0.4, false),
    ];
    let recency = RecencyRing::new(20);
    // Use a target_energy that scores both b and c equally on the energy axis.
    let mut sel = Selector::new(&library, &recency, 0.4);
    let current = &library[0];

    // Pick repeatedly; b should be chosen more often than c because b would produce a
    // long-blend transition (higher transition_score) while c would produce a 16-bar fade.
    let mut b_wins = 0;
    let mut c_wins = 0;
    for _ in 0..200 {
        let pick = sel.pick_next(current).unwrap();
        if pick.sidecar.source_path == "b" {
            b_wins += 1;
        } else if pick.sidecar.source_path == "c" {
            c_wins += 1;
        }
    }
    assert!(
        b_wins > c_wins,
        "expected b (long blend) > c (fade); got b={} c={}",
        b_wins,
        c_wins
    );
}
