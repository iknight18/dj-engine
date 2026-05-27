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
use crate::ffmpeg::{CHANNELS, SAMPLE_RATE};

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

use std::collections::VecDeque;

use crate::automation::{Automation, AutomationQueue};

pub struct Deck {
    id: DeckId,
    state: DeckState,
    automations: AutomationQueue,
    sources: VecDeque<DeckSource>,
    /// Master-clock frame count this deck has emitted.
    frame_cursor: u64,
    /// Current source-time position in seconds (used when respawning decoders).
    source_position_seconds: f64,
    /// cue_out boundary in source-time seconds; respawns clamp to this.
    source_end_seconds: f64,
}

impl Deck {
    pub fn new(id: DeckId) -> Self {
        Self {
            id,
            state: DeckState::default(),
            automations: AutomationQueue::new(),
            sources: VecDeque::new(),
            frame_cursor: 0,
            source_position_seconds: 0.0,
            source_end_seconds: f64::INFINITY,
        }
    }

    pub fn id(&self) -> DeckId {
        self.id
    }

    pub fn state(&self) -> &DeckState {
        &self.state
    }

    pub fn frame_cursor(&self) -> u64 {
        self.frame_cursor
    }

    pub fn schedule(&mut self, a: Automation) {
        self.automations.schedule(a);
    }

    pub async fn unload(&mut self) {
        self.sources.clear();
        self.state.source_path = None;
        self.state.play_state = PlayState::Stopped;
        self.state.fader = 0.0;
        self.state.tempo = 1.0;
        self.source_position_seconds = 0.0;
        self.source_end_seconds = f64::INFINITY;
    }
}

use crate::library::LoadedTrack;
use std::path::PathBuf;

impl Deck {
    /// Load a track and spawn the decoder positioned at `source_start_seconds`.
    /// After this call: Cueing, fader=0, tempo=1.0 (caller sets tempo via automation).
    pub async fn cue(
        &mut self,
        track: LoadedTrack,
        source_start_seconds: f64,
    ) -> Result<()> {
        let path: PathBuf = track.sidecar.source_path.clone().into();
        let cue_out = track
            .sidecar
            .cue_out_seconds
            .unwrap_or(track.sidecar.duration_seconds.unwrap_or(f64::INFINITY));
        let decoder = PlayingDecoder::spawn(&path, source_start_seconds, cue_out).await?;
        self.sources.clear();
        self.sources.push_back(DeckSource::Decoder(decoder));
        self.state.source_path = Some(track.sidecar.source_path.clone());
        self.state.play_state = PlayState::Cueing;
        self.state.fader = 0.0;
        self.state.tempo = 1.0;
        self.source_position_seconds = source_start_seconds;
        self.source_end_seconds = cue_out;
        Ok(())
    }

    pub fn play(&mut self) {
        if matches!(self.state.play_state, PlayState::Cueing) {
            self.state.play_state = PlayState::Playing;
        }
    }

    /// Pull `n_frames` of interleaved stereo PCM. Always advances `frame_cursor`
    /// by `n_frames`. Returns zeros for Stopped/Cueing.
    pub async fn pull(&mut self, n_frames: usize) -> Result<Vec<f32>> {
        self.frame_cursor += n_frames as u64;

        if !matches!(self.state.play_state, PlayState::Playing) {
            return Ok(vec![0.0; n_frames * (CHANNELS as usize)]);
        }

        // Drain from the source queue until we have n_frames or run dry.
        let need_samples = n_frames * (CHANNELS as usize);
        let mut out: Vec<f32> = Vec::with_capacity(need_samples);
        while out.len() < need_samples {
            let remaining_frames = (need_samples - out.len()) / (CHANNELS as usize);
            if remaining_frames == 0 {
                break;
            }
            let Some(source) = self.sources.front_mut() else { break };
            let chunk = source.pull(remaining_frames).await?;
            if chunk.is_empty() {
                // Source ran dry; advance.
                self.sources.pop_front();
                continue;
            }
            out.extend_from_slice(&chunk);
            if source.is_exhausted() {
                self.sources.pop_front();
            }
        }
        // Pad with silence if the queue is empty.
        if out.len() < need_samples {
            out.resize(need_samples, 0.0);
        }

        // Apply current fader as a constant scalar over this tick. Task 13
        // makes this per-sample by evaluating automations at sub-tick resolution.
        let g = self.state.fader;
        if g != 1.0 {
            for s in out.iter_mut() {
                *s *= g;
            }
        }

        // Advance source-position estimate (used by tempo respawns in Task 14).
        let emitted_seconds = n_frames as f64 / SAMPLE_RATE as f64;
        self.source_position_seconds += emitted_seconds * self.state.tempo;

        Ok(out)
    }

    /// Test-only fader setter — bypasses the automation queue.
    #[doc(hidden)]
    pub fn test_set_fader(&mut self, v: f32) {
        self.state.fader = v;
    }
}
