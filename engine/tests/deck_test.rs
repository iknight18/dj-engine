use dj_engine::automation::{Automation, AutomationTarget, Curve};
use dj_engine::deck::{Deck, DeckId, PlayState};
use dj_engine::library::LoadedTrack;
use dj_engine::sidecar::Sidecar;
use std::path::PathBuf;

fn fixture_track() -> LoadedTrack {
    // Use a tiny WAV fixture committed under tests/fixtures/.
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sine_440_3s.wav");
    LoadedTrack {
        sidecar_path: PathBuf::new(),
        sidecar: Sidecar {
            schema_version: 1,
            analyzer_version: "test".into(),
            source_path: path.to_string_lossy().into_owned(),
            source_mtime: 0.0,
            source_size_bytes: 0,
            duration_seconds: Some(3.0),
            sample_rate: Some(44100),
            bpm: Some(120.0),
            bpm_confidence: Some(1.0),
            beats_seconds: vec![],
            downbeats_seconds: vec![],
            key: None,
            key_label: None,
            key_confidence: None,
            energy_curve: vec![],
            energy_mean: Some(0.5),
            band_energy_means: None,
            cue_in_seconds: Some(0.0),
            cue_out_seconds: Some(3.0),
            intro_end_seconds: Some(0.0),
            outro_start_seconds: Some(3.0),
            is_mixable: Some(true),
            mixable_reasons: vec![],
            error: None,
            phrase_boundaries_seconds: None,
            phrase_confidence: None,
            band_energy_curves: None,
            is_beatmatchable: None,
            is_phrase_aligned: None,
            stems: None,
        },
    }
}

#[test]
fn new_deck_is_stopped_with_zero_fader_unity_tempo() {
    let d = Deck::new(DeckId::A);
    assert_eq!(d.state().play_state, PlayState::Stopped);
    assert_eq!(d.state().fader, 0.0);
    assert!((d.state().tempo - 1.0).abs() < 1e-9);
    assert_eq!(d.id(), DeckId::A);
    assert_eq!(d.frame_cursor(), 0);
}

#[tokio::test]
async fn unload_resets_state() {
    let mut d = Deck::new(DeckId::B);
    d.unload().await;
    assert_eq!(d.state().play_state, PlayState::Stopped);
    assert!(d.state().source_path.is_none());
}

#[tokio::test]
async fn cue_transitions_to_cueing_state() {
    let mut d = Deck::new(DeckId::A);
    let track = fixture_track();
    d.cue(track, 0.0, 1.0).await.expect("cue ok");
    assert_eq!(d.state().play_state, PlayState::Cueing);
    assert!(d.state().source_path.is_some());
}

#[tokio::test]
async fn pull_stopped_returns_zeros() {
    let mut d = Deck::new(DeckId::A);
    let pcm = d.pull(1024).await.expect("pull ok");
    assert_eq!(pcm.len(), 1024 * 2);
    assert!(pcm.iter().all(|s| *s == 0.0));
    assert_eq!(d.frame_cursor(), 1024);
}

#[tokio::test]
async fn pull_cueing_returns_zeros() {
    let mut d = Deck::new(DeckId::A);
    d.cue(fixture_track(), 0.0, 1.0).await.unwrap();
    let pcm = d.pull(512).await.unwrap();
    assert_eq!(pcm.len(), 512 * 2);
    assert!(pcm.iter().all(|s| *s == 0.0));
    assert_eq!(d.frame_cursor(), 512);
    // Still in Cueing because play() hasn't been called.
    assert_eq!(d.state().play_state, PlayState::Cueing);
}

#[tokio::test]
async fn play_transitions_to_playing() {
    let mut d = Deck::new(DeckId::A);
    d.cue(fixture_track(), 0.0, 1.0).await.unwrap();
    d.play();
    assert_eq!(d.state().play_state, PlayState::Playing);
}

#[tokio::test]
async fn pull_playing_produces_nonzero_with_fader_unity() {
    let mut d = Deck::new(DeckId::A);
    d.cue(fixture_track(), 0.0, 1.0).await.unwrap();
    d.play();
    // Manually set fader=1.0 (Task 13 adds automation-driven setting).
    // We use the test helper added below to poke state.
    d.test_set_fader(1.0);
    let pcm = d.pull(4096).await.unwrap();
    assert_eq!(pcm.len(), 4096 * 2);
    let energy: f32 = pcm.iter().map(|s| s.abs()).sum();
    assert!(
        energy > 1.0,
        "expected non-trivial audio, got energy {energy}"
    );
}

#[tokio::test]
async fn pull_playing_with_fader_zero_returns_silence() {
    let mut d = Deck::new(DeckId::A);
    d.cue(fixture_track(), 0.0, 1.0).await.unwrap();
    d.play();
    // fader stays at 0.0 (its initial value)
    let pcm = d.pull(4096).await.unwrap();
    assert!(pcm.iter().all(|s| *s == 0.0));
}

#[tokio::test]
async fn tempo_linear_ramp_swaps_to_post_ramp_decoder() {
    // Cue a 6-second test fixture.
    let mut d = Deck::new(DeckId::A);
    let track = LoadedTrack {
        sidecar_path: PathBuf::new(),
        sidecar: Sidecar {
            source_path: format!(
                "{}/tests/fixtures/sine_440_6s.wav",
                env!("CARGO_MANIFEST_DIR")
            ),
            duration_seconds: Some(6.0),
            cue_out_seconds: Some(6.0),
            ..fixture_track().sidecar
        },
    };
    d.cue(track, 0.0, 1.0).await.unwrap();
    d.play();
    d.test_set_fader(1.0);
    // Schedule a Tempo Step from 1.07 → 1.07 (no-op, set the deck's tempo state).
    d.schedule(Automation {
        target: AutomationTarget::Tempo,
        t_start_frames: 0,
        t_end_frames: 1,
        from: 1.07,
        to: 1.07,
        curve: Curve::Step,
    });
    // Schedule a Linear ramp 1.07 → 1.0 over the next ~2 seconds.
    d.schedule(Automation {
        target: AutomationTarget::Tempo,
        t_start_frames: 0,
        t_end_frames: 88_200, // ~2s at 44.1 kHz
        from: 1.07,
        to: 1.0,
        curve: Curve::Linear,
    });
    // Pull through a few ticks; the deck should produce continuous audio.
    let mut total: Vec<f32> = Vec::new();
    for _ in 0..30 {
        let chunk = d.pull(4096).await.unwrap();
        total.extend_from_slice(&chunk);
    }
    // Check no long gap of silence (> 1024 frames in a row of zeros = bug).
    let mut zero_run = 0usize;
    let mut max_zero_run = 0usize;
    for s in &total {
        if *s == 0.0 {
            zero_run += 1;
            max_zero_run = max_zero_run.max(zero_run);
        } else {
            zero_run = 0;
        }
    }
    assert!(
        max_zero_run < 1024 * 2,
        "long silence gap: {max_zero_run} samples"
    );
}

fn rms_stereo(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let s: f32 = samples.iter().map(|x| x * x).sum();
    (s / samples.len() as f32).sqrt()
}

#[tokio::test]
async fn eq_low_step_minus_40_kills_low_frequency_audio() {
    // Setup: cue a fixture, schedule EqLow Step to -40 dB before any audio plays.
    let mut d = Deck::new(DeckId::A);
    d.cue(fixture_track(), 0.0, 1.0).await.unwrap();
    d.play();
    d.test_set_fader(1.0);
    d.schedule(Automation {
        target: AutomationTarget::EqLow,
        t_start_frames: 0,
        t_end_frames: 1,
        from: -40.0,
        to: -40.0,
        curve: Curve::Step,
    });
    // Drop the first tick (decoder priming) so we're well into the body of the
    // 440 Hz fixture before measuring. Then measure RMS of two ticks of audio.
    let _ = d.pull(4096).await.unwrap();
    let kill_low = d.pull(8192).await.unwrap();
    let r_killed = rms_stereo(&kill_low);

    // Tear down and rerun with NO EQ automation to get the baseline RMS.
    let mut d2 = Deck::new(DeckId::A);
    d2.cue(fixture_track(), 0.0, 1.0).await.unwrap();
    d2.play();
    d2.test_set_fader(1.0);
    let _ = d2.pull(4096).await.unwrap();
    let baseline = d2.pull(8192).await.unwrap();
    let r_baseline = rms_stereo(&baseline);

    // 440 Hz is above the 200 Hz low-shelf knee but the shelf still has SOME
    // effect (it's a shelf, not a brick wall). Expect the killed RMS to be
    // noticeably (>15%) lower than baseline.
    assert!(
        r_killed < r_baseline * 0.85,
        "expected EqLow=-40dB to attenuate; killed {r_killed} vs baseline {r_baseline}"
    );
    assert!((d.state().eq_low_db - (-40.0)).abs() < 1e-9);
}

#[tokio::test]
async fn filter_step_plus_1_attenuates_low_frequency() {
    // Filter at +1 (full HP at 8 kHz) should attenuate the 440 Hz fixture.
    let mut d = Deck::new(DeckId::A);
    d.cue(fixture_track(), 0.0, 1.0).await.unwrap();
    d.play();
    d.test_set_fader(1.0);
    d.schedule(Automation {
        target: AutomationTarget::Filter,
        t_start_frames: 0,
        t_end_frames: 1,
        from: 1.0,
        to: 1.0,
        curve: Curve::Step,
    });
    // Skip the first tick (decoder priming + biquad startup transient).
    let _ = d.pull(4096).await.unwrap();
    let filtered = d.pull(8192).await.unwrap();
    let r_filtered = rms_stereo(&filtered);

    let mut d2 = Deck::new(DeckId::A);
    d2.cue(fixture_track(), 0.0, 1.0).await.unwrap();
    d2.play();
    d2.test_set_fader(1.0);
    let _ = d2.pull(4096).await.unwrap();
    let baseline = d2.pull(8192).await.unwrap();
    let r_baseline = rms_stereo(&baseline);

    assert!(
        r_filtered < r_baseline * 0.6,
        "HP at 8 kHz should attenuate 440 Hz; filtered {r_filtered} vs baseline {r_baseline}"
    );
    assert!((d.state().filter_knob - 1.0).abs() < 1e-9);
}

#[tokio::test]
async fn filter_bypass_at_zero_passes_through_unchanged() {
    let mut d = Deck::new(DeckId::A);
    d.cue(fixture_track(), 0.0, 1.0).await.unwrap();
    d.play();
    d.test_set_fader(1.0);
    // No filter automation - knob stays at 0.
    let _ = d.pull(4096).await.unwrap();
    let out_filtered = d.pull(8192).await.unwrap();
    let r_filtered = rms_stereo(&out_filtered);

    let mut d2 = Deck::new(DeckId::A);
    d2.cue(fixture_track(), 0.0, 1.0).await.unwrap();
    d2.play();
    d2.test_set_fader(1.0);
    let _ = d2.pull(4096).await.unwrap();
    let baseline = d2.pull(8192).await.unwrap();
    let r_baseline = rms_stereo(&baseline);

    // With knob = 0, output should be equivalent to baseline (no filter applied).
    assert!(
        (r_filtered - r_baseline).abs() < r_baseline * 0.01,
        "knob=0 should be passthrough; got {r_filtered} vs {r_baseline}"
    );
    assert!(d.state().filter_knob.abs() < f64::EPSILON);
}

#[tokio::test]
async fn cue_resets_filter_knob() {
    let mut d = Deck::new(DeckId::A);
    d.cue(fixture_track(), 0.0, 1.0).await.unwrap();
    d.play();
    d.schedule(Automation {
        target: AutomationTarget::Filter,
        t_start_frames: 0,
        t_end_frames: 1,
        from: 0.0,
        to: 1.0,
        curve: Curve::Step,
    });
    let _ = d.pull(4096).await.unwrap();
    assert!((d.state().filter_knob - 1.0).abs() < 1e-9);
    d.cue(fixture_track(), 0.0, 1.0).await.unwrap();
    assert!(d.state().filter_knob.abs() < f64::EPSILON);
}

#[tokio::test]
async fn cue_resets_echo_and_loop_state() {
    let mut d = Deck::new(DeckId::A);
    d.cue(fixture_track(), 0.0, 1.0).await.unwrap();
    d.set_echo_params(0.5, 0.5);
    d.set_loop_bars(4, 0.5);
    // Re-cue resets effect state.
    d.cue(fixture_track(), 0.0, 1.0).await.unwrap();
    assert!(d.state().echo_mix.abs() < f64::EPSILON);
    assert!(d.state().loop_gate.abs() < f64::EPSILON);
}

#[tokio::test]
async fn echo_mix_one_produces_audible_tail() {
    // With echo configured and EchoMix=1, the post-fader output should contain
    // non-zero energy after the fader hits 0 (i.e., echo tails ring through).
    let mut d = Deck::new(DeckId::A);
    d.cue(fixture_track(), 0.0, 1.0).await.unwrap();
    d.set_echo_params(0.05, 0.8); // 50 ms delay, heavy feedback for audible tail
    d.play();
    d.test_set_fader(1.0);
    d.schedule(Automation {
        target: AutomationTarget::EchoMix,
        t_start_frames: 0,
        t_end_frames: 1,
        from: 1.0,
        to: 1.0,
        curve: Curve::Step,
    });
    // Pull ~250 ms of dry+wet, then drop the fader to 0 and pull 100 ms more —
    // expect the second block to have non-trivial RMS (the echo tail).
    let _ = d.pull(11025).await.unwrap(); // ~250ms
    d.test_set_fader(0.0);
    let tail = d.pull(4410).await.unwrap(); // ~100ms
    let rms = rms_stereo(&tail);
    // Echo line has the previous 250ms of energy in its buffer; the wet output
    // should be audibly non-zero even with the dry path silenced.
    assert!(rms > 0.001, "expected non-trivial tail, got rms {rms}");
}

#[tokio::test]
async fn loop_gate_freezes_source_position() {
    let mut d = Deck::new(DeckId::A);
    d.cue(fixture_track(), 0.0, 1.0).await.unwrap();
    d.set_loop_bars(1, 0.5); // 1 bar at 0.5 s/bar = 0.5 s loop
    d.play();
    d.test_set_fader(1.0);
    // Run for ~1 s to fill the capture buffer.
    let _ = d.pull(44100).await.unwrap();
    let pos_before_lock = d.source_position_seconds();
    // Lock the loop.
    d.schedule(Automation {
        target: AutomationTarget::LoopGate,
        t_start_frames: d.frame_cursor(),
        t_end_frames: d.frame_cursor() + 1,
        from: 1.0,
        to: 1.0,
        curve: Curve::Step,
    });
    // Pull another ~1 s — source_position_seconds should NOT advance.
    let _ = d.pull(44100).await.unwrap();
    let pos_after_lock = d.source_position_seconds();
    assert!(
        (pos_after_lock - pos_before_lock).abs() < 0.01,
        "source position should freeze; before={pos_before_lock} after={pos_after_lock}"
    );
}

#[test]
fn stems_dir_for_resolves_sibling_stems_directory() {
    use dj_engine::deck::stems_dir_for;

    // Build a temp filesystem layout: analyzed/foo.mp3.json + analyzed-stems/foo.mp3/{drums,bass,vocals,other}.mp3
    let tmp = std::env::temp_dir().join(format!(
        "djengine-stems-dir-test-{}",
        std::process::id()
    ));
    let analyzed = tmp.join("analyzed");
    let stems_root = tmp.join("analyzed-stems");
    let track_dir = stems_root.join("foo.mp3");
    std::fs::create_dir_all(&analyzed).unwrap();
    std::fs::create_dir_all(&track_dir).unwrap();
    let sidecar_path = analyzed.join("foo.mp3.json");
    std::fs::write(&sidecar_path, "{}").unwrap();

    let resolved = stems_dir_for(&sidecar_path).expect("should resolve");
    assert_eq!(resolved, track_dir);

    // Cleanup (best-effort).
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn stems_dir_for_returns_none_when_missing() {
    use dj_engine::deck::stems_dir_for;
    let path = std::env::temp_dir()
        .join("djengine-nonexistent-analyzed")
        .join("nope.mp3.json");
    assert!(stems_dir_for(&path).is_none());
}

#[tokio::test]
async fn cue_stems_returns_error_when_stems_directory_absent() {
    let mut d = Deck::new(DeckId::A);
    // fixture_track's sidecar_path is empty PathBuf so stems_dir_for returns None.
    let track = fixture_track();
    let res = d.cue_stems(track, 0.0, 1.0).await;
    assert!(res.is_err(), "expected StemsUnavailable error");
}

#[tokio::test]
async fn fader_linear_automation_ramps_signal() {
    let mut d = Deck::new(DeckId::A);
    d.cue(fixture_track(), 0.0, 1.0).await.unwrap();
    d.play();
    // Schedule a Linear 0→1 fader over the first 8192 frames.
    d.schedule(Automation {
        target: AutomationTarget::Fader,
        t_start_frames: 0,
        t_end_frames: 8192,
        from: 0.0,
        to: 1.0,
        curve: Curve::Linear,
    });
    // Pull half the ramp.
    let first = d.pull(4096).await.unwrap();
    let energy_first: f32 = first.iter().map(|s| s.abs()).sum();
    // Pull the rest of the ramp.
    let second = d.pull(4096).await.unwrap();
    let energy_second: f32 = second.iter().map(|s| s.abs()).sum();
    // Second half should be louder (ramp 0→1 means fader avg is ~0.25 then ~0.75).
    assert!(
        energy_second > energy_first * 2.0,
        "expected second half louder; got first={energy_first} second={energy_second}"
    );
}
