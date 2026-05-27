//! Mixer: orchestrates decode → align → crossfade → ring buffer.

use tracing::info;

use crate::buffer::{Producer, SampleProducer};
use crate::crossfade::{mix_crossfade, mix_crossfade_eq};
use crate::error::Result;
use crate::ffmpeg::decoder::PlayingDecoder;
use crate::ffmpeg::mixwindow::decode_stretched;
use crate::ffmpeg::SAMPLE_RATE;
use crate::library::LoadedTrack;
use crate::transition::{plan_transition, TransitionPlan};

const READ_CHUNK_FRAMES: usize = 4096;

async fn push_all(producer: &mut SampleProducer, samples: &[f32]) {
    let mut offset = 0;
    while offset < samples.len() {
        let n = producer.push_slice(&samples[offset..]);
        if n == 0 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        } else {
            offset += n;
        }
    }
}

/// Run a single transition from `outgoing` to `incoming`. Returns the new playing-decoder.
pub async fn play_until_transition_and_crossfade(
    outgoing: &LoadedTrack,
    incoming: &LoadedTrack,
    decoder: &mut PlayingDecoder,
    producer: &mut SampleProducer,
) -> Result<PlayingDecoder> {
    let plan = plan_transition(&outgoing.sidecar, &incoming.sidecar);

    if plan.length_bars == 0 {
        // Hard cut (Task 10).
        return execute_hard_cut(outgoing, incoming, decoder, producer, &plan).await;
    }

    execute_crossfade(outgoing, incoming, decoder, producer, &plan).await
}

async fn execute_crossfade(
    outgoing: &LoadedTrack,
    incoming: &LoadedTrack,
    decoder: &mut PlayingDecoder,
    producer: &mut SampleProducer,
    plan: &TransitionPlan,
) -> Result<PlayingDecoder> {
    let outgoing_bpm = outgoing.sidecar.bpm.unwrap_or(120.0);
    let incoming_bpm = incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
    let ratio = outgoing_bpm / incoming_bpm.max(1.0);

    let bar_seconds = (60.0 / outgoing_bpm) * 4.0;
    let crossfade_seconds = plan.length_bars as f64 * bar_seconds;

    let incoming_path: std::path::PathBuf = incoming.sidecar.source_path.clone().into();

    // Pre-fetch a stretched window around the incoming crossfade-start point.
    // We grab from start - 1 bar (small head room) to start + length_bars (the crossfade region).
    let stretched_start = (plan.incoming_crossfade_start_seconds - bar_seconds).max(0.0);
    let stretched_end =
        plan.incoming_crossfade_start_seconds + crossfade_seconds + bar_seconds;
    let stretched =
        decode_stretched(&incoming_path, stretched_start, stretched_end, ratio).await?;

    // The crossfade audio inside `stretched` starts at offset (incoming_crossfade_start - stretched_start)
    // in track-time seconds; the stretched buffer has 1x sample rate after atempo, so this offset
    // in samples is just (offset_seconds * SR * 2) — no rate correction needed because atempo preserves SR.
    let offset_seconds = plan.incoming_crossfade_start_seconds - stretched_start;
    let offset_samples = (offset_seconds * SAMPLE_RATE as f64) as usize * 2;
    let crossfade_frames = (crossfade_seconds * SAMPLE_RATE as f64) as usize;
    let crossfade_samples = crossfade_frames * 2;

    let incoming_head_end = (offset_samples + crossfade_samples).min(stretched.len());
    let mut incoming_head = stretched[offset_samples.min(stretched.len())..incoming_head_end].to_vec();
    if incoming_head.len() < crossfade_samples {
        incoming_head.resize(crossfade_samples, 0.0);
    }

    // 1) Stream outgoing PCM until the decoder's output cursor reaches outgoing_crossfade_start.
    let target_seconds =
        (plan.outgoing_crossfade_start_seconds - decoder.start_seconds).max(0.0);
    let target_samples = (target_seconds * SAMPLE_RATE as f64) as usize * 2;
    let mut pushed: usize = 0;

    while pushed < target_samples {
        let pcm = decoder.read_frames(READ_CHUNK_FRAMES).await?;
        if pcm.is_empty() {
            break;
        }
        let want = target_samples - pushed;
        let to_push = pcm.len().min(want);
        push_all(producer, &pcm[..to_push]).await;
        pushed += to_push;
    }

    // 2) Read the crossfade tail of outgoing.
    let mut outgoing_tail = decoder.read_frames(crossfade_frames).await?;
    if outgoing_tail.len() < crossfade_samples {
        outgoing_tail.resize(crossfade_samples, 0.0);
    }

    // 3) Mix.
    let mixed = if plan.use_eq_swap {
        mix_crossfade_eq(&outgoing_tail, &incoming_head, crossfade_frames, SAMPLE_RATE as f64)
    } else {
        mix_crossfade(&outgoing_tail, &incoming_head, crossfade_frames)
    };
    push_all(producer, &mixed).await;

    info!(
        outgoing = %outgoing.sidecar.source_path,
        incoming = %incoming.sidecar.source_path,
        length_bars = plan.length_bars,
        eq = plan.use_eq_swap,
        "crossfade complete"
    );

    // 4) Spawn the new playing decoder past the crossfade region on the incoming side.
    let new_start = plan.incoming_crossfade_start_seconds + crossfade_seconds;
    let cue_out = incoming.sidecar.cue_out_seconds.unwrap_or(f64::INFINITY);
    let new_decoder = PlayingDecoder::spawn(&incoming_path, new_start, cue_out).await?;
    Ok(new_decoder)
}

/// Placeholder — implemented in Task 10.
async fn execute_hard_cut(
    _outgoing: &LoadedTrack,
    _incoming: &LoadedTrack,
    _decoder: &mut PlayingDecoder,
    _producer: &mut SampleProducer,
    _plan: &TransitionPlan,
) -> Result<PlayingDecoder> {
    unimplemented!("hard cut path — Task 10")
}
