//! Mixer: orchestrates decode → align → crossfade → ring buffer.

use tracing::info;

use crate::buffer::{Producer, SampleProducer};
use crate::crossfade::{mix_crossfade, mix_crossfade_eq};
use crate::error::Result;
use crate::ffmpeg::decoder::PlayingDecoder;
use crate::ffmpeg::mixwindow::{
    decode_stretched, decode_stretched_ramp, ramp_source_consumed_seconds,
    ATEMPO_PER_CALL_OVERHEAD_S,
};
use crate::ffmpeg::SAMPLE_RATE;
use crate::library::LoadedTrack;
use crate::transition::{plan_transition, TransitionPlan};

const READ_CHUNK_FRAMES: usize = 4096;

/// Bars of post-crossfade pitch-journey ramp: incoming track's atempo ratio linearly
/// returns to 1.0 over this many bars so the swap to the native-tempo decoder is not
/// audible. See docs/superpowers/specs/2026-05-27-pitch-journey-design.md.
const RAMP_BARS: usize = 16;
/// Output duration per ramp chunk in bars (RAMP_BARS / RAMP_CHUNK_BARS = chunk count).
/// Each chunk = one ffmpeg+atempo invocation. Each call has ~130ms of WSOLA overhead
/// in source-consumption (see ATEMPO_PER_CALL_OVERHEAD_S), which is compensated when
/// computing the new-decoder spawn point.
const RAMP_CHUNK_BARS: usize = 4;
/// Below this |ratio - 1.0|, skip the ramp entirely — matches mixwindow's atempo bypass.
const RAMP_IDENTITY_EPSILON: f64 = 0.005;
/// Length of the splice crossfade between the ramp's tail and the new playing
/// decoder's first samples. Hides any small (sample-to-beat-scale) misalignment
/// that the atempo-overhead compensation didn't perfectly cancel.
const SPLICE_FADE_MS: f64 = 300.0;

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
    // We grab from start - 1 bar (small head room) to start + crossfade + 1 bar of post-roll
    // (used to bridge ffmpeg subprocess startup latency on the new playing decoder).
    let stretched_start = (plan.incoming_crossfade_start_seconds - bar_seconds).max(0.0);
    let stretched_end = plan.incoming_crossfade_start_seconds + crossfade_seconds + bar_seconds;

    // `decode_stretched` slices its output in OUTPUT-time, so the post-roll's true
    // source-end is `stretched_end * ratio` (atempo at ratio R consumes R seconds of
    // source per second of output), plus ~130ms per-call WSOLA overhead. When atempo
    // is bypassed (ratio ~ 1.0), source-time = output-time and there's no overhead.
    let post_roll_source_end = if (ratio - 1.0).abs() < RAMP_IDENTITY_EPSILON {
        stretched_end
    } else {
        stretched_end * ratio + ATEMPO_PER_CALL_OVERHEAD_S
    };

    // Pitch journey: after the crossfade + post-roll, ramp the incoming track's atempo
    // ratio from `ratio` back toward 1.0 over RAMP_BARS so the swap to the native-tempo
    // decoder is not audible. Skip when the ratio is already ~1.0 (nothing to relax).
    let ramp_skipped = (ratio - 1.0).abs() < RAMP_IDENTITY_EPSILON;
    let cue_out_inc = incoming.sidecar.cue_out_seconds.unwrap_or(f64::INFINITY);
    let nominal_ramp_seconds = if ramp_skipped {
        0.0
    } else {
        RAMP_BARS as f64 * bar_seconds
    };
    // Clamp ramp to the source remaining before cue_out — if the clamped duration
    // is less than 1 bar, skip the ramp entirely rather than emit a stubby one.
    let max_ramp_source = (cue_out_inc - post_roll_source_end).max(0.0);
    let max_ramp_output = max_ramp_source / ((ratio + 1.0) / 2.0).max(0.001);
    let effective_ramp_seconds = if ramp_skipped {
        0.0
    } else {
        nominal_ramp_seconds.min(max_ramp_output)
    };
    let ramp_chunks = if effective_ramp_seconds >= bar_seconds {
        (RAMP_BARS / RAMP_CHUNK_BARS).max(1)
    } else {
        0
    };
    let ramp_source_consumed = if ramp_chunks > 0 {
        ramp_source_consumed_seconds(ratio, 1.0, effective_ramp_seconds, ramp_chunks)
    } else {
        0.0
    };

    // Start spawning the new playing decoder NOW, in parallel with the stretched decode
    // and the rest of the crossfade work. It starts SPLICE_FADE_MS BEFORE the predicted
    // ramp end so the splice crossfade later overlaps source content (rather than mixing
    // two different parts of the song, which sounds like a stutter).
    let splice_fade_seconds = SPLICE_FADE_MS / 1000.0;
    let new_start =
        (post_roll_source_end + ramp_source_consumed - splice_fade_seconds).max(0.0);
    let cue_out = cue_out_inc;
    let new_decoder_task = {
        let path = incoming_path.clone();
        tokio::spawn(async move { PlayingDecoder::spawn(&path, new_start, cue_out).await })
    };

    let stretched = decode_stretched(&incoming_path, stretched_start, stretched_end, ratio).await?;

    // The crossfade audio inside `stretched` starts at offset (incoming_crossfade_start - stretched_start)
    // in track-time seconds; the stretched buffer has 1x sample rate after atempo, so this offset
    // in samples is just (offset_seconds * SR * 2) — no rate correction needed because atempo preserves SR.
    let offset_seconds = plan.incoming_crossfade_start_seconds - stretched_start;
    let offset_samples = (offset_seconds * SAMPLE_RATE as f64) as usize * 2;
    let crossfade_frames = (crossfade_seconds * SAMPLE_RATE as f64) as usize;
    let crossfade_samples = crossfade_frames * 2;

    let incoming_head_end = (offset_samples + crossfade_samples).min(stretched.len());
    let mut incoming_head =
        stretched[offset_samples.min(stretched.len())..incoming_head_end].to_vec();
    if incoming_head.len() < crossfade_samples {
        incoming_head.resize(crossfade_samples, 0.0);
    }

    // 1) Stream outgoing PCM until the decoder's output cursor reaches outgoing_crossfade_start.
    let target_seconds = (plan.outgoing_crossfade_start_seconds - decoder.start_seconds).max(0.0);
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
    let bass_hint = match (
        outgoing.sidecar.band_energy_means.as_ref(),
        incoming.sidecar.band_energy_means.as_ref(),
    ) {
        (Some(o), Some(i)) => Some(crate::crossfade::BassMixHint {
            outgoing_bass_mean: o.bass,
            incoming_bass_mean: i.bass,
        }),
        _ => None,
    };

    let mixed = if plan.use_eq_swap {
        mix_crossfade_eq(
            &outgoing_tail,
            &incoming_head,
            crossfade_frames,
            SAMPLE_RATE as f64,
            bass_hint,
        )
    } else {
        mix_crossfade(&outgoing_tail, &incoming_head, crossfade_frames)
    };
    push_all(producer, &mixed).await;

    // 4) Push the post-crossfade tail from the stretched buffer (1 bar, the headroom we
    // pre-decoded). This bridges the seam between the crossfade audio and the ramp.
    let post_crossfade_start = (offset_samples + crossfade_samples).min(stretched.len());
    if post_crossfade_start < stretched.len() {
        push_all(producer, &stretched[post_crossfade_start..]).await;
    }

    // 5) Pitch journey ramp — gradually relax atempo from `ratio` toward 1.0 over
    // RAMP_BARS so the swap to the native-tempo decoder is not audible.
    let ramp_pcm = if ramp_chunks > 0 {
        decode_stretched_ramp(
            &incoming_path,
            post_roll_source_end,
            ratio,
            1.0,
            effective_ramp_seconds,
            ramp_chunks,
        )
        .await?
    } else {
        Vec::new()
    };

    // 6) Collect the pre-spawned new decoder.
    let mut new_decoder = new_decoder_task
        .await
        .map_err(|e| crate::error::EngineError::Ffmpeg(format!("decoder task join: {e}")))??;

    // 7) Splice the ramp's tail and the new decoder's head with a short equal-power
    // crossfade. atempo's per-call source-consumption overhead is variable (~50-200ms),
    // so even with our best-effort compensation the seam can have small misalignment in
    // either direction. A 300 ms fade hides it.
    let splice_frames = ((SPLICE_FADE_MS / 1000.0) * SAMPLE_RATE as f64) as usize;
    let splice_samples = splice_frames * 2;

    if !ramp_pcm.is_empty() && ramp_pcm.len() >= splice_samples {
        // Push everything in the ramp except its last SPLICE_FADE_MS.
        let body_end = ramp_pcm.len() - splice_samples;
        push_all(producer, &ramp_pcm[..body_end]).await;

        // Take the ramp's tail.
        let mut ramp_tail = ramp_pcm[body_end..].to_vec();
        // Read the same number of samples from the new decoder.
        let mut new_head = new_decoder.read_frames(splice_frames).await?;
        if new_head.len() < splice_samples {
            new_head.resize(splice_samples, 0.0);
        }
        if ramp_tail.len() < splice_samples {
            ramp_tail.resize(splice_samples, 0.0);
        }
        let spliced = mix_crossfade(&ramp_tail, &new_head, splice_frames);
        push_all(producer, &spliced).await;
        // The new decoder's cursor is now SPLICE_FADE_MS into the playback —
        // advance its start_seconds accordingly so the next iteration's "stream until
        // outgoing_crossfade_start" math is correct.
        new_decoder.start_seconds += SPLICE_FADE_MS / 1000.0;
    } else if !ramp_pcm.is_empty() {
        // Ramp shorter than the fade duration (rare): push it whole, no splice.
        push_all(producer, &ramp_pcm).await;
    }

    let ramp_bars_logged = if ramp_chunks > 0 {
        (effective_ramp_seconds / bar_seconds).round() as usize
    } else {
        0
    };
    info!(
        outgoing = %outgoing.sidecar.source_path,
        incoming = %incoming.sidecar.source_path,
        length_bars = plan.length_bars,
        eq = plan.use_eq_swap,
        ramp_bars = ramp_bars_logged,
        "crossfade complete"
    );
    Ok(new_decoder)
}

/// Hard cut: play outgoing up to the snapped boundary, then splice incoming's first frame
/// directly with no overlap and no volume blending.
async fn execute_hard_cut(
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

    let incoming_path: std::path::PathBuf = incoming.sidecar.source_path.clone().into();

    // 1) Stream outgoing until we reach the snapped boundary, exactly the same as the
    //    crossfade case — but no crossfade region after.
    let target_seconds = (plan.outgoing_crossfade_start_seconds - decoder.start_seconds).max(0.0);
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

    // 2) Pre-fetch a tiny bit of the incoming track at the snapped start, time-stretched.
    //    We grab 1 bar so the splice has audio ready, then spawn the long-running decoder
    //    to take over for the rest.
    let stretched_start = plan.incoming_crossfade_start_seconds;
    let stretched_end = plan.incoming_crossfade_start_seconds + bar_seconds;
    let stretched = decode_stretched(&incoming_path, stretched_start, stretched_end, ratio).await?;
    push_all(producer, &stretched).await;

    info!(
        outgoing = %outgoing.sidecar.source_path,
        incoming = %incoming.sidecar.source_path,
        "hard cut complete"
    );

    // 3) Spawn the new playing decoder past the bar we just spliced.
    let new_start = stretched_end;
    let cue_out = incoming.sidecar.cue_out_seconds.unwrap_or(f64::INFINITY);
    let new_decoder = PlayingDecoder::spawn(&incoming_path, new_start, cue_out).await?;
    Ok(new_decoder)
}
