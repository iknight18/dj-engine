//! Mixer: orchestrates decode → align → crossfade → ring buffer.

use tracing::info;

use crate::buffer::{Producer, SampleProducer};
use crate::crossfade::mix_crossfade;
use crate::error::Result;
use crate::ffmpeg::decoder::PlayingDecoder;
use crate::ffmpeg::mixwindow::decode_stretched;
use crate::ffmpeg::SAMPLE_RATE;
use crate::library::LoadedTrack;

const READ_CHUNK_FRAMES: usize = 4096;

/// Push every sample in `samples` to the producer, blocking (via short sleeps) until
/// every sample has been accepted. This is the real backpressure mechanism — without it,
/// the ring buffer silently drops audio when full.
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

/// Run a single crossfade transition from `outgoing` to `incoming` and update the
/// "now playing" decoder reference. Returns the new playing-decoder once incoming is current.
pub async fn play_until_transition_and_crossfade(
    outgoing: &LoadedTrack,
    incoming: &LoadedTrack,
    decoder: &mut PlayingDecoder,
    producer: &mut SampleProducer,
) -> Result<PlayingDecoder> {
    let outgoing_outro_start = outgoing.sidecar.outro_start_seconds.unwrap_or(0.0);
    let outgoing_bpm = outgoing.sidecar.bpm.unwrap_or(120.0);
    let incoming_bpm = incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
    let ratio = outgoing_bpm / incoming_bpm.max(1.0);

    let incoming_intro_end = incoming.sidecar.intro_end_seconds.unwrap_or(16.0);
    let bar_seconds = (60.0 / outgoing_bpm) * 4.0;
    let crossfade_seconds = (16.0 * bar_seconds).min(32.0);

    // Pre-fetch the incoming track's mix window (time-stretched in advance).
    // We grab from intro_end - 16 bars to intro_end + 16 bars — the first half is what
    // we mix during the crossfade; the second half is what the new playing decoder
    // continues from after the crossfade ends.
    let incoming_path: std::path::PathBuf = incoming.sidecar.source_path.clone().into();
    let stretched = decode_stretched(
        &incoming_path,
        (incoming_intro_end - (16.0 * bar_seconds)).max(0.0),
        incoming_intro_end + (16.0 * bar_seconds),
        ratio,
    )
    .await?;

    // 1) Stream outgoing PCM until we've pushed enough audio to reach outro_start_seconds
    //    of the OUTGOING track. The decoder's output cursor advances from
    //    `decoder.start_seconds` forward; we want to push until that cursor is at
    //    outgoing_outro_start.
    let target_seconds = (outgoing_outro_start - decoder.start_seconds).max(0.0);
    let target_samples = (target_seconds * SAMPLE_RATE as f64) as usize * 2;
    let mut pushed: usize = 0;

    while pushed < target_samples {
        let pcm = decoder.read_frames(READ_CHUNK_FRAMES).await?;
        if pcm.is_empty() {
            break; // outgoing track ended early; crossfade now
        }
        let want = target_samples - pushed;
        let to_push = pcm.len().min(want);
        push_all(producer, &pcm[..to_push]).await;
        pushed += to_push;
    }

    // 2) Crossfade: blend the next 16 bars of outgoing with the first 16 bars of stretched.
    let crossfade_samples = (crossfade_seconds * SAMPLE_RATE as f64) as usize * 2;
    let crossfade_frames = crossfade_samples / 2;

    let mut outgoing_tail = decoder.read_frames(crossfade_frames).await?;
    // If outgoing tail is short (track ended), pad with silence so the crossfade math
    // still produces `crossfade_samples` of output.
    if outgoing_tail.len() < crossfade_samples {
        outgoing_tail.resize(crossfade_samples, 0.0);
    }

    let mut incoming_head = stretched.clone();
    if incoming_head.len() < crossfade_samples {
        incoming_head.resize(crossfade_samples, 0.0);
    } else {
        incoming_head.truncate(crossfade_samples);
    }

    let mixed = mix_crossfade(&outgoing_tail, &incoming_head, crossfade_frames);
    push_all(producer, &mixed).await;

    info!(
        outgoing = %outgoing.sidecar.source_path,
        incoming = %incoming.sidecar.source_path,
        crossfade_seconds,
        "crossfade complete"
    );

    // 3) Spawn the new playing decoder positioned right where the crossfade left off:
    //    the listener has heard the incoming track up through `intro_end` (16 bars of
    //    stretched starting at intro_end - 16 bars lands at intro_end). The new decoder
    //    picks up from intro_end forward.
    let cue_out = incoming.sidecar.cue_out_seconds.unwrap_or(f64::INFINITY);
    let new_decoder = PlayingDecoder::spawn(&incoming_path, incoming_intro_end, cue_out).await?;

    Ok(new_decoder)
}
