use dj_engine::buffer::make_ring;
use dj_engine::deck::DeckId;
use dj_engine::ffmpeg::SAMPLE_RATE;
use dj_engine::library::LoadedTrack;
use dj_engine::mixer::Mixer;
use dj_engine::recipe::EqualPowerCrossfade;
use dj_engine::scheduler::TransitionScheduler;
use dj_engine::sidecar::Sidecar;
use dj_engine::transition::TransitionPlan;
use std::path::PathBuf;

fn fixture_track(name: &str, dur: f64) -> LoadedTrack {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/{name}"));
    LoadedTrack {
        sidecar_path: PathBuf::new(),
        sidecar: Sidecar {
            schema_version: 1,
            analyzer_version: "test".into(),
            source_path: path.to_string_lossy().into_owned(),
            source_mtime: 0.0,
            source_size_bytes: 0,
            duration_seconds: Some(dur),
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
            cue_out_seconds: Some(dur),
            intro_end_seconds: Some(0.0),
            outro_start_seconds: Some(dur),
            is_mixable: Some(true),
            mixable_reasons: vec![],
            error: None,
            phrase_boundaries_seconds: None,
            phrase_confidence: None,
            band_energy_curves: None,
            is_beatmatchable: None,
            is_phrase_aligned: None,
        },
    }
}

#[tokio::test]
async fn arm_cues_incoming_and_schedules_automations() {
    let cap = (SAMPLE_RATE as usize) * 2 * 10;
    let (producer, _consumer) = make_ring(cap);
    let mut mixer = Mixer::new(producer);
    let outgoing = fixture_track("sine_440_6s.wav", 6.0);
    let incoming = fixture_track("sine_660_6s.wav", 6.0);
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 0.0,
        incoming_crossfade_start_seconds: 2.0,
        length_bars: 16,
        use_eq_swap: false,
    };
    let handle = TransitionScheduler::arm(
        &mut mixer,
        DeckId::A,
        DeckId::B,
        &outgoing,
        &incoming,
        &plan,
        &EqualPowerCrossfade,
    )
    .await
    .expect("arm");

    let bar_seconds = (60.0_f64 / 120.0) * 4.0;
    let expected = (plan.length_bars as f64 * bar_seconds * SAMPLE_RATE as f64) as u64;
    assert!(
        handle.end_master_clock_frames >= expected,
        "end {} < expected {expected}",
        handle.end_master_clock_frames
    );
    assert_eq!(handle.recipe_name, "equal_power_crossfade");
}
