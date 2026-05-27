//! Deck: one persistent playback channel with its own decoder + state.

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub enum DeckId {
    A,
    B,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayState {
    Stopped,
    Cueing,
    Playing,
}

#[derive(Clone, Debug)]
pub struct DeckState {
    pub fader: f32,
    pub tempo: f64,
    pub source_path: Option<String>,
    pub play_state: PlayState,
}

impl Default for DeckState {
    fn default() -> Self {
        Self {
            fader: 0.0,
            tempo: 1.0,
            source_path: None,
            play_state: PlayState::Stopped,
        }
    }
}
