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
    pub eq_low_db: f64,
    pub eq_mid_db: f64,
    pub eq_high_db: f64,
    pub filter_knob: f64,
    pub echo_mix: f64,
    pub loop_gate: f64,
    pub source_path: Option<String>,
    pub play_state: PlayState,
}

impl Default for DeckState {
    fn default() -> Self {
        Self {
            fader: 0.0,
            tempo: 1.0,
            eq_low_db: 0.0,
            eq_mid_db: 0.0,
            eq_high_db: 0.0,
            filter_knob: 0.0,
            echo_mix: 0.0,
            loop_gate: 0.0,
            source_path: None,
            play_state: PlayState::Stopped,
        }
    }
}

const EQ_LOW_FREQ: f64 = 200.0;
const EQ_MID_FREQ: f64 = 1000.0;
const EQ_MID_Q: f64 = 1.0;
const EQ_HIGH_FREQ: f64 = 5000.0;

// Low-pass: knob 0 -> 22 kHz (passthrough), knob -1 -> 200 Hz.
const FILTER_LP_FREQ_BYPASS: f64 = 22_000.0;
const FILTER_LP_FREQ_MAX_LEFT: f64 = 200.0;

// High-pass: knob 0 -> 10 Hz (passthrough), knob +1 -> 8 kHz.
const FILTER_HP_FREQ_BYPASS: f64 = 10.0;
const FILTER_HP_FREQ_MAX_RIGHT: f64 = 8_000.0;

// Below this absolute knob value, skip the filter entirely (no biquad evaluation).
const FILTER_BYPASS_EPSILON: f64 = 0.01;

// Worst-case allocations for per-deck effects. Echo delay is bounded by 2 s
// (covers a full beat at down to 30 BPM); loop capture is bounded by 8 s (covers
// 8 bars at down to 30 BPM, or 4 bars at down to 60 BPM).
const ECHO_CAPACITY_SAMPLES: usize = (SAMPLE_RATE as usize) * 2;
const LOOP_CAPACITY_SAMPLES: usize = (SAMPLE_RATE as usize) * 8;

/// Map a knob value in `[-1, +1]` to a `(FilterKind, cutoff_hz)` pair using a
/// log-Hz lerp from the bypass frequency to the max-deflection frequency.
/// Knob >= 0 -> high-pass, knob < 0 -> low-pass.
fn filter_cutoff_hz(knob: f64) -> (crate::eq::FilterKind, f64) {
    if knob >= 0.0 {
        (
            crate::eq::FilterKind::HighPass,
            log_lerp(FILTER_HP_FREQ_BYPASS, FILTER_HP_FREQ_MAX_RIGHT, knob),
        )
    } else {
        (
            crate::eq::FilterKind::LowPass,
            log_lerp(FILTER_LP_FREQ_BYPASS, FILTER_LP_FREQ_MAX_LEFT, -knob),
        )
    }
}

fn log_lerp(from: f64, to: f64, t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    (from.ln() + (to.ln() - from.ln()) * t).exp()
}

use crate::error::Result;
use crate::ffmpeg::decoder::PlayingDecoder;
use crate::ffmpeg::{CHANNELS, SAMPLE_RATE};

/// One PCM source within a deck. The deck plays through a queue of these,
/// advancing to the next when the current is exhausted.
pub(crate) enum DeckSource {
    /// Long-running ffmpeg decoder (with or without atempo). Boxed because
    /// `PlayingDecoder` (containing tokio `Child` + `ChildStdout`) is large
    /// compared to the `Buffer` variant.
    Decoder(Box<PlayingDecoder>),
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
            DeckSource::Buffer {
                pcm,
                offset_samples,
            } => {
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
            DeckSource::Buffer {
                pcm,
                offset_samples,
            } => *offset_samples >= pcm.len(),
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
    /// `t_start_frames` of the Tempo Linear ramp we've already materialized
    /// into the source queue. `None` means no ramp staged yet.
    ramp_materialized_for: Option<u64>,
    eq_low_l: crate::eq::Biquad,
    eq_low_r: crate::eq::Biquad,
    eq_mid_l: crate::eq::Biquad,
    eq_mid_r: crate::eq::Biquad,
    eq_high_l: crate::eq::Biquad,
    eq_high_r: crate::eq::Biquad,
    last_eq_low_db: f64,
    last_eq_mid_db: f64,
    last_eq_high_db: f64,
    filter_l: crate::eq::Biquad,
    filter_r: crate::eq::Biquad,
    last_filter_knob: f64,
    echo_line: crate::effects::EchoLine,
    loop_buffer: crate::effects::LoopBuffer,
}

impl Deck {
    pub fn new(id: DeckId) -> Self {
        let sr = SAMPLE_RATE as f64;
        Self {
            id,
            state: DeckState::default(),
            automations: AutomationQueue::new(),
            sources: VecDeque::new(),
            frame_cursor: 0,
            source_position_seconds: 0.0,
            source_end_seconds: f64::INFINITY,
            ramp_materialized_for: None,
            eq_low_l: crate::eq::Biquad::low_shelf(sr, EQ_LOW_FREQ, 0.0),
            eq_low_r: crate::eq::Biquad::low_shelf(sr, EQ_LOW_FREQ, 0.0),
            eq_mid_l: crate::eq::Biquad::peaking(sr, EQ_MID_FREQ, EQ_MID_Q, 0.0),
            eq_mid_r: crate::eq::Biquad::peaking(sr, EQ_MID_FREQ, EQ_MID_Q, 0.0),
            eq_high_l: crate::eq::Biquad::high_shelf(sr, EQ_HIGH_FREQ, 0.0),
            eq_high_r: crate::eq::Biquad::high_shelf(sr, EQ_HIGH_FREQ, 0.0),
            last_eq_low_db: 0.0,
            last_eq_mid_db: 0.0,
            last_eq_high_db: 0.0,
            filter_l: crate::eq::Biquad::lowpass(sr, FILTER_LP_FREQ_BYPASS),
            filter_r: crate::eq::Biquad::lowpass(sr, FILTER_LP_FREQ_BYPASS),
            last_filter_knob: 0.0,
            echo_line: crate::effects::EchoLine::new(ECHO_CAPACITY_SAMPLES),
            loop_buffer: crate::effects::LoopBuffer::new(LOOP_CAPACITY_SAMPLES),
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

    /// Current source-time position in seconds. Advances by `emitted_seconds *
    /// tempo` on each pull while Playing. Used by the main loop to schedule the
    /// next transition based on where the deck actually is, not where it would
    /// be if it had started at `cue_in`.
    pub fn source_position_seconds(&self) -> f64 {
        self.source_position_seconds
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
        self.ramp_materialized_for = None;
        self.reset_eq();
        self.reset_filter();
        self.reset_effects();
    }

    /// Reset the per-deck EQ chain to passthrough (0 dB on all three bands).
    /// Used by `cue` and `unload` so a new track starts with a clean EQ state.
    fn reset_eq(&mut self) {
        self.state.eq_low_db = 0.0;
        self.state.eq_mid_db = 0.0;
        self.state.eq_high_db = 0.0;
        self.last_eq_low_db = 0.0;
        self.last_eq_mid_db = 0.0;
        self.last_eq_high_db = 0.0;
        let sr = SAMPLE_RATE as f64;
        self.eq_low_l = crate::eq::Biquad::low_shelf(sr, EQ_LOW_FREQ, 0.0);
        self.eq_low_r = crate::eq::Biquad::low_shelf(sr, EQ_LOW_FREQ, 0.0);
        self.eq_mid_l = crate::eq::Biquad::peaking(sr, EQ_MID_FREQ, EQ_MID_Q, 0.0);
        self.eq_mid_r = crate::eq::Biquad::peaking(sr, EQ_MID_FREQ, EQ_MID_Q, 0.0);
        self.eq_high_l = crate::eq::Biquad::high_shelf(sr, EQ_HIGH_FREQ, 0.0);
        self.eq_high_r = crate::eq::Biquad::high_shelf(sr, EQ_HIGH_FREQ, 0.0);
    }

    /// Reset the per-deck one-knob filter to bypass (knob = 0) and re-initialize
    /// both biquads as a passthrough low-pass. Used by `cue` and `unload` so a
    /// new track starts with no filtering.
    fn reset_filter(&mut self) {
        self.state.filter_knob = 0.0;
        self.last_filter_knob = 0.0;
        let sr = SAMPLE_RATE as f64;
        self.filter_l = crate::eq::Biquad::lowpass(sr, FILTER_LP_FREQ_BYPASS);
        self.filter_r = crate::eq::Biquad::lowpass(sr, FILTER_LP_FREQ_BYPASS);
    }

    /// Reset the per-deck echo + loop effects to a silent, unconfigured state.
    /// Used by `cue` and `unload` so a new track starts with no echo tails and
    /// no captured loop content.
    fn reset_effects(&mut self) {
        self.echo_line.clear();
        self.echo_line.set_params(0, 0.0);
        self.loop_buffer.clear();
        self.state.echo_mix = 0.0;
        self.state.loop_gate = 0.0;
    }

    /// Configure the echo delay line's static parameters. Called by the
    /// scheduler from `DeckInitConfig` after `cue()` and before `play()`.
    pub fn set_echo_params(&mut self, delay_seconds: f64, feedback: f64) {
        let delay_samples = (delay_seconds * SAMPLE_RATE as f64) as usize;
        self.echo_line.set_params(delay_samples, feedback);
    }

    /// Configure the loop buffer's active window length in bars. Called by the
    /// scheduler from `DeckInitConfig` after `cue()` and before `play()`.
    pub fn set_loop_bars(&mut self, bars: usize, bar_seconds: f64) {
        let samples = (bars as f64 * bar_seconds * SAMPLE_RATE as f64) as usize;
        self.loop_buffer.set_loop_samples(samples);
    }
}

use crate::library::LoadedTrack;
use std::path::PathBuf;

impl Deck {
    /// Load a track and spawn the decoder positioned at `source_start_seconds`,
    /// with `tempo` applied as an atempo ratio. `tempo=1.0` plays at native rate
    /// (no atempo). After this call: Cueing, fader=0, tempo=tempo. The deck's
    /// `state.tempo` matches the spawned decoder so `source_position_seconds`
    /// tracking stays consistent with the decoder's actual source consumption.
    pub async fn cue(
        &mut self,
        track: LoadedTrack,
        source_start_seconds: f64,
        tempo: f64,
    ) -> Result<()> {
        let path: PathBuf = track.sidecar.source_path.clone().into();
        let cue_out = track
            .sidecar
            .cue_out_seconds
            .unwrap_or(track.sidecar.duration_seconds.unwrap_or(f64::INFINITY));
        let decoder =
            PlayingDecoder::spawn_with_tempo(&path, source_start_seconds, cue_out, tempo).await?;
        self.sources.clear();
        self.sources
            .push_back(DeckSource::Decoder(Box::new(decoder)));
        self.state.source_path = Some(track.sidecar.source_path.clone());
        self.state.play_state = PlayState::Cueing;
        self.state.fader = 0.0;
        self.state.tempo = tempo;
        self.source_position_seconds = source_start_seconds;
        self.source_end_seconds = cue_out;
        self.ramp_materialized_for = None;
        self.reset_eq();
        self.reset_filter();
        self.reset_effects();
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
        let start_cursor = self.frame_cursor;
        self.maybe_prepare_tempo_ramp(start_cursor).await?;
        self.frame_cursor += n_frames as u64;

        if !matches!(self.state.play_state, PlayState::Playing) {
            // Still tick the automation queue so future automations land correctly.
            self.automations.step(self.frame_cursor, &mut self.state);
            return Ok(vec![0.0; n_frames * (CHANNELS as usize)]);
        }

        // Pre-step: apply any automations at start_cursor so EQ / filter / loop
        // transitions reflect the current automation state before we decide.
        self.automations.step(start_cursor, &mut self.state);
        self.recompute_eq_if_dirty();
        self.recompute_filter_if_dirty();

        // Loop transitions (boundary-trigger semantics at tick start).
        if self.state.loop_gate >= 0.5 && !self.loop_buffer.is_locked() {
            self.loop_buffer.lock();
        } else if self.state.loop_gate < 0.5 && self.loop_buffer.is_locked() {
            self.loop_buffer.unlock();
        }
        let loop_active = self.loop_buffer.is_locked();
        let filter_active = self.state.filter_knob.abs() >= FILTER_BYPASS_EPSILON;

        let need_samples = n_frames * (CHANNELS as usize);
        let mut out: Vec<f32> = Vec::with_capacity(need_samples);

        if loop_active {
            // Fill `out` from the locked loop buffer; do NOT drain the source.
            for _ in 0..n_frames {
                let (l, r) = self.loop_buffer.read();
                out.push(l);
                out.push(r);
            }
        } else {
            // Drain source queue (existing logic).
            while out.len() < need_samples {
                let remaining_frames = (need_samples - out.len()) / (CHANNELS as usize);
                if remaining_frames == 0 {
                    break;
                }
                let Some(source) = self.sources.front_mut() else {
                    break;
                };
                let chunk = source.pull(remaining_frames).await?;
                if chunk.is_empty() {
                    self.sources.pop_front();
                    continue;
                }
                out.extend_from_slice(&chunk);
                if source.is_exhausted() {
                    self.sources.pop_front();
                }
            }
            if out.len() < need_samples {
                out.resize(need_samples, 0.0);
            }
        }

        // Per-sample chain: tick automations, capture into loop buffer (only when
        // not locked, to avoid self-feedback), EQ → filter → fader → echo. The
        // echo line is fed by the *pre-fader* post-filter signal so its tail
        // continues to ring even when the fader closes; the wet output is then
        // mixed in on top of the dry post-fader signal.
        for f in 0..n_frames {
            let cursor = start_cursor + f as u64;
            self.automations.step(cursor, &mut self.state);

            let li = f * 2;
            let ri = li + 1;
            let raw_l = out[li];
            let raw_r = out[ri];

            // Capture only while not locked (avoid self-feedback into the loop buffer).
            if !loop_active {
                self.loop_buffer.write(raw_l, raw_r);
            }

            // EQ
            let l = self
                .eq_high_l
                .process(self.eq_mid_l.process(self.eq_low_l.process(raw_l)));
            let r = self
                .eq_high_r
                .process(self.eq_mid_r.process(self.eq_low_r.process(raw_r)));

            // Filter (one-knob)
            let (l, r) = if filter_active {
                (self.filter_l.process(l), self.filter_r.process(r))
            } else {
                (l, r)
            };

            // Fader (dry path).
            let g = self.state.fader;
            let dry_l = l * g;
            let dry_r = r * g;

            // Echo: pre-fader signal into the line; line's wet output added on top.
            let (wet_l, wet_r) = self.echo_line.process(l, r);
            let mix = self.state.echo_mix as f32;
            out[li] = dry_l + wet_l * mix;
            out[ri] = dry_r + wet_r * mix;
        }
        // Final step at frame_cursor (after the loop) to install any to-values
        // for automations that finish exactly at frame_cursor.
        self.automations.step(self.frame_cursor, &mut self.state);

        // Advance source position only when NOT in loop mode. Looping freezes the
        // playhead so the recipe can rejoin from where the lock happened.
        if !loop_active {
            let emitted_seconds = n_frames as f64 / SAMPLE_RATE as f64;
            self.source_position_seconds += emitted_seconds * self.state.tempo;
        }
        Ok(out)
    }

    /// Update biquad coefficients in place when any band's dB target has
    /// changed since the last evaluation. Cheap when state is stable: a few
    /// f64 compares per sample, no allocation, no DSP work.
    fn recompute_eq_if_dirty(&mut self) {
        let sr = SAMPLE_RATE as f64;
        if (self.state.eq_low_db - self.last_eq_low_db).abs() > f64::EPSILON {
            self.eq_low_l
                .set_low_shelf(sr, EQ_LOW_FREQ, self.state.eq_low_db);
            self.eq_low_r
                .set_low_shelf(sr, EQ_LOW_FREQ, self.state.eq_low_db);
            self.last_eq_low_db = self.state.eq_low_db;
        }
        if (self.state.eq_mid_db - self.last_eq_mid_db).abs() > f64::EPSILON {
            self.eq_mid_l
                .set_peaking(sr, EQ_MID_FREQ, EQ_MID_Q, self.state.eq_mid_db);
            self.eq_mid_r
                .set_peaking(sr, EQ_MID_FREQ, EQ_MID_Q, self.state.eq_mid_db);
            self.last_eq_mid_db = self.state.eq_mid_db;
        }
        if (self.state.eq_high_db - self.last_eq_high_db).abs() > f64::EPSILON {
            self.eq_high_l
                .set_high_shelf(sr, EQ_HIGH_FREQ, self.state.eq_high_db);
            self.eq_high_r
                .set_high_shelf(sr, EQ_HIGH_FREQ, self.state.eq_high_db);
            self.last_eq_high_db = self.state.eq_high_db;
        }
    }

    /// Update the one-knob filter's biquad coefficients in place when the knob
    /// has changed. Inside the bypass dead-zone (`|knob| < FILTER_BYPASS_EPSILON`)
    /// we still set coefficients to a harmless passthrough LP at the bypass
    /// frequency, but the pull loop skips the biquad altogether so those
    /// coefficients are not evaluated.
    fn recompute_filter_if_dirty(&mut self) {
        if (self.state.filter_knob - self.last_filter_knob).abs() <= f64::EPSILON {
            return;
        }
        let sr = SAMPLE_RATE as f64;
        if self.state.filter_knob.abs() < FILTER_BYPASS_EPSILON {
            self.filter_l
                .set_cutoff(sr, crate::eq::FilterKind::LowPass, FILTER_LP_FREQ_BYPASS);
            self.filter_r
                .set_cutoff(sr, crate::eq::FilterKind::LowPass, FILTER_LP_FREQ_BYPASS);
        } else {
            let (kind, cutoff) = filter_cutoff_hz(self.state.filter_knob);
            self.filter_l.set_cutoff(sr, kind, cutoff);
            self.filter_r.set_cutoff(sr, kind, cutoff);
        }
        self.last_filter_knob = self.state.filter_knob;
    }

    /// Test-only fader setter — bypasses the automation queue.
    #[doc(hidden)]
    pub fn test_set_fader(&mut self, v: f32) {
        self.state.fader = v;
    }
}

const SPLICE_FADE_MS: f64 = 300.0;
const RAMP_CHUNK_BARS: usize = 4;
const RAMP_BARS: usize = 16;

mod stretch {
    //! Internal helpers: chunked atempo decode for the pitch-journey ramp.
    //! (Formerly engine/src/ffmpeg/mixwindow.rs; inlined here so the deck is
    //! self-contained.)

    use std::path::Path;
    use std::process::Stdio;

    use tokio::io::AsyncReadExt;
    use tokio::process::Command;

    use crate::error::{EngineError, Result};
    use crate::ffmpeg::{CHANNELS, SAMPLE_RATE};

    /// Empirically-measured WSOLA source-consumption overhead per `atempo` ffmpeg
    /// invocation (MP3 source, ratios near 1.0). Each call consumes ~130ms more
    /// source than `output * ratio` would predict.
    pub const ATEMPO_PER_CALL_OVERHEAD_S: f64 = 0.13;

    /// Decode `output_seconds` of audio from `source_path` starting at source-time
    /// `source_start_seconds`, time-stretched by `ratio`. Uses input seek
    /// (`-ss before -i`) for source positioning and `-t output_seconds` to bound
    /// output duration -- so the source range consumed is
    /// `[source_start, source_start + output_seconds * ratio]` and the returned
    /// buffer is exactly `output_seconds` of PCM at SAMPLE_RATE.
    pub async fn decode_atempo_at_source(
        source_path: &Path,
        source_start_seconds: f64,
        output_seconds: f64,
        ratio: f64,
    ) -> Result<Vec<f32>> {
        if ratio <= 0.0 {
            return Err(EngineError::Ffmpeg(format!(
                "invalid atempo ratio: {ratio}"
            )));
        }
        if output_seconds <= 0.0 {
            return Ok(Vec::new());
        }

        let identity = (ratio - 1.0).abs() < 0.005;

        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-hide_banner").arg("-loglevel").arg("error");
        cmd.arg("-accurate_seek")
            .arg("-ss")
            .arg(format!("{:.6}", source_start_seconds.max(0.0)))
            .arg("-i")
            .arg(source_path)
            .arg("-t")
            .arg(format!("{:.6}", output_seconds));

        if !identity {
            cmd.arg("-af").arg(atempo_chain(ratio));
        }

        cmd.arg("-f")
            .arg("f32le")
            .arg("-ar")
            .arg(SAMPLE_RATE.to_string())
            .arg("-ac")
            .arg(CHANNELS.to_string())
            .arg("-")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| EngineError::Ffmpeg(format!("spawn: {e}")))?;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| EngineError::Ffmpeg("no stdout".into()))?;
        let mut buf = Vec::new();
        stdout
            .read_to_end(&mut buf)
            .await
            .map_err(EngineError::Io)?;

        let status = child.wait().await.map_err(EngineError::Io)?;
        if !status.success() {
            return Err(EngineError::Ffmpeg(format!("ffmpeg exited with {status}")));
        }

        let mut samples = Vec::with_capacity(buf.len() / 4);
        for chunk in buf.chunks_exact(4) {
            samples.push(f32::from_le_bytes(chunk.try_into().unwrap()));
        }
        Ok(samples)
    }

    /// Decode a ramp region of `source_path`, starting at source-time
    /// `source_start_seconds`, producing `ramp_seconds` of stereo PCM output.
    /// The atempo ratio is linearly interpolated between `start_ratio` and
    /// `end_ratio` across `chunks` equal-output-duration chunks (midpoint
    /// interpolation: chunk `i` uses `lerp(start, end, (i + 0.5) / chunks)`).
    pub async fn decode_stretched_ramp(
        source_path: &Path,
        source_start_seconds: f64,
        start_ratio: f64,
        end_ratio: f64,
        ramp_seconds: f64,
        chunks: usize,
    ) -> Result<Vec<f32>> {
        if chunks == 0 || ramp_seconds <= 0.0 {
            return Ok(Vec::new());
        }
        let chunk_out_seconds = ramp_seconds / chunks as f64;
        let mut out = Vec::new();
        let mut src_pos = source_start_seconds;
        for i in 0..chunks {
            let frac = (i as f64 + 0.5) / chunks as f64;
            let ratio = start_ratio + (end_ratio - start_ratio) * frac;
            let chunk =
                decode_atempo_at_source(source_path, src_pos, chunk_out_seconds, ratio).await?;
            out.extend_from_slice(&chunk);
            src_pos += chunk_out_seconds * ratio + ATEMPO_PER_CALL_OVERHEAD_S;
        }
        Ok(out)
    }

    /// Source-time consumed by a ramp with the given parameters.
    pub fn ramp_source_consumed_seconds(
        start_ratio: f64,
        end_ratio: f64,
        ramp_seconds: f64,
        chunks: usize,
    ) -> f64 {
        ramp_seconds * (start_ratio + end_ratio) / 2.0
            + (chunks as f64) * ATEMPO_PER_CALL_OVERHEAD_S
    }

    /// Build an atempo filter chain -- atempo only accepts ratios in [0.5, 2.0],
    /// so for ratios outside that we chain multiple atempo instances.
    fn atempo_chain(ratio: f64) -> String {
        if (0.5..=2.0).contains(&ratio) {
            return format!("atempo={ratio}");
        }
        let mut remaining = ratio;
        let mut parts = Vec::new();
        while remaining > 2.0 {
            parts.push(2.0_f64);
            remaining /= 2.0;
        }
        while remaining < 0.5 {
            parts.push(0.5_f64);
            remaining /= 0.5;
        }
        parts.push(remaining);
        parts
            .iter()
            .map(|f| format!("atempo={f}"))
            .collect::<Vec<_>>()
            .join(",")
    }
}

impl Deck {
    /// Look for a pending Tempo Linear automation that we have not yet
    /// materialized into source-queue PCM. If found, build the ramp buffer +
    /// post-ramp decoder + splice and push them onto the queue.
    async fn maybe_prepare_tempo_ramp(&mut self, cursor: u64) -> Result<()> {
        // Look up the next pending Tempo Linear ramp; abort if there is none, if
        // it has already been materialized, or if it is still further than one
        // bar away.
        let (ramp_t_start, ramp_from, ramp_to, ramp_frames, one_bar_frames) = {
            let Some(ramp) = self.automations.peek_pending_tempo_linear() else {
                return Ok(());
            };
            if self.ramp_materialized_for == Some(ramp.t_start_frames) {
                return Ok(());
            }
            let bar_seconds = 60.0_f64 / 120.0 * 4.0;
            let one_bar = (SAMPLE_RATE as f64 * bar_seconds) as u64;
            if ramp.t_start_frames > cursor + one_bar {
                return Ok(());
            }
            (
                ramp.t_start_frames,
                ramp.from,
                ramp.to,
                ramp.t_end_frames - ramp.t_start_frames,
                one_bar,
            )
        };
        let _ = one_bar_frames;

        let source_path: std::path::PathBuf = self
            .state
            .source_path
            .as_ref()
            .map(|s| s.into())
            .ok_or_else(|| crate::error::EngineError::Ffmpeg("no source".into()))?;
        let start_ratio = ramp_from;
        let end_ratio = ramp_to;
        let ramp_seconds = ramp_frames as f64 / SAMPLE_RATE as f64;
        let chunks = (RAMP_BARS / RAMP_CHUNK_BARS).max(1);

        // Source-time position where the ramp begins.
        let ramp_source_start = self.source_position_seconds;
        let ramp_consumed =
            stretch::ramp_source_consumed_seconds(start_ratio, end_ratio, ramp_seconds, chunks);
        let post_ramp_source = (ramp_source_start + ramp_consumed).min(self.source_end_seconds);

        // Decode the ramp PCM (blocking await — runs ahead of real time).
        let mut ramp_pcm = stretch::decode_stretched_ramp(
            &source_path,
            ramp_source_start,
            start_ratio,
            end_ratio,
            ramp_seconds,
            chunks,
        )
        .await?;

        // Pre-spawn the post-ramp decoder and read SPLICE_FADE_MS of its head.
        let splice_frames = ((SPLICE_FADE_MS / 1000.0) * SAMPLE_RATE as f64) as usize;
        let splice_samples = splice_frames * (CHANNELS as usize);
        let new_start = (post_ramp_source - SPLICE_FADE_MS / 1000.0).max(0.0);
        let mut new_decoder =
            PlayingDecoder::spawn(&source_path, new_start, self.source_end_seconds).await?;
        let mut new_head = new_decoder.read_frames(splice_frames).await?;
        if new_head.len() < splice_samples {
            new_head.resize(splice_samples, 0.0);
        }
        // Splice: replace ramp_pcm's last splice_samples with an equal-power
        // crossfade between ramp_tail and new_head.
        if ramp_pcm.len() >= splice_samples {
            use std::f64::consts::FRAC_PI_2;
            let body_end = ramp_pcm.len() - splice_samples;
            for i in 0..splice_samples {
                let t = (i / 2) as f64 / splice_frames.max(1) as f64;
                let g_out = (t * FRAC_PI_2).cos();
                let g_in = (t * FRAC_PI_2).sin();
                let ramp_sample = ramp_pcm[body_end + i] as f64;
                let new_sample = new_head[i] as f64;
                ramp_pcm[body_end + i] = (ramp_sample * g_out + new_sample * g_in) as f32;
            }
        }

        // Clear the existing source queue (the steady-state decoder for the
        // pre-ramp portion has already been pulled through; whatever is left
        // is residual). Push buffer, then post-ramp decoder.
        self.sources.clear();
        self.sources.push_back(DeckSource::Buffer {
            pcm: ramp_pcm,
            offset_samples: 0,
        });
        self.sources
            .push_back(DeckSource::Decoder(Box::new(new_decoder)));

        // Update source position to reflect that we've now committed to
        // post-ramp playback.
        self.source_position_seconds = new_start + (SPLICE_FADE_MS / 1000.0);
        self.ramp_materialized_for = Some(ramp_t_start);
        Ok(())
    }
}
