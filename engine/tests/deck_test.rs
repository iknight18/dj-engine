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
    d.cue(track, 0.0).await.expect("cue ok");
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
    d.cue(fixture_track(), 0.0).await.unwrap();
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
    d.cue(fixture_track(), 0.0).await.unwrap();
    d.play();
    assert_eq!(d.state().play_state, PlayState::Playing);
}
