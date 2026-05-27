use dj_engine::deck::{Deck, DeckId, PlayState};

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
