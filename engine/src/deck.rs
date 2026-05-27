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

use crate::error::Result;
use crate::ffmpeg::decoder::PlayingDecoder;
use crate::ffmpeg::CHANNELS;

/// One PCM source within a deck. The deck plays through a queue of these,
/// advancing to the next when the current is exhausted.
pub(crate) enum DeckSource {
    /// Long-running ffmpeg decoder (with or without atempo).
    Decoder(PlayingDecoder),
    /// Pre-decoded PCM (used for the chunked atempo ramp + splice tail).
    Buffer {
        pcm: Vec<f32>,
        offset_samples: usize,
    },
}

impl DeckSource {
    /// Pull up to `n_frames` of interleaved stereo PCM. Returns a buffer that
    /// may be shorter than `n_frames * 2` if the source is exhausted.
    pub(crate) async fn pull(&mut self, n_frames: usize) -> Result<Vec<f32>> {
        match self {
            DeckSource::Decoder(decoder) => decoder.read_frames(n_frames).await,
            DeckSource::Buffer { pcm, offset_samples } => {
                let need_samples = n_frames * (CHANNELS as usize);
                let end = (*offset_samples + need_samples).min(pcm.len());
                let out = pcm[*offset_samples..end].to_vec();
                *offset_samples = end;
                Ok(out)
            }
        }
    }

    pub(crate) fn is_exhausted(&self) -> bool {
        match self {
            DeckSource::Decoder(_) => false, // can't tell without a read; treated as never exhausted
            DeckSource::Buffer { pcm, offset_samples } => *offset_samples >= pcm.len(),
        }
    }
}
