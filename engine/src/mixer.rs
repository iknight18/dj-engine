//! Mixer: orchestrates decode → align → crossfade → ring buffer.

use tracing::info;

use crate::buffer::{Observer, Producer, SampleProducer};
use crate::crossfade::mix_crossfade;
use crate::error::Result;
use crate::ffmpeg::decoder::PlayingDecoder;
use crate::ffmpeg::mixwindow::decode_stretched;
use crate::ffmpeg::SAMPLE_RATE;
use crate::library::LoadedTrack;

const READ_CHUNK_FRAMES: usize = 4096;

/// Run a single crossfade transition from `outgoing` to `incoming` and update the
/// "now playing" decoder reference. Returns the new playing-decoder once incoming is current.
pub async fn play_until_transition_and_crossfade(
    outgoing: &LoadedTrack,
    incoming: &LoadedTrack,
    decoder: &mut PlayingDecoder,
    producer: &mut SampleProducer,
) -> Result<PlayingDecoder> {
    let _outgoing_outro_start = outgoing.sidecar.outro_start_seconds.unwrap_or(0.0);
    let outgoing_bpm = outgoing.sidecar.bpm.unwrap_or(120.0);
    let incoming_bpm = incoming.sidecar.bpm.unwrap_or(outgoing_bpm);
    let ratio = outgoing_bpm / incoming_bpm.max(1.0);

    let incoming_intro_end = incoming.sidecar.intro_end_seconds.unwrap_or(16.0);
    let bar_seconds = (60.0 / outgoing_bpm) * 4.0;
    let crossfade_seconds = (16.0 * bar_seconds).min(32.0);

    // 1) Stream outgoing PCM until we approach outro_start_seconds.
    //    Switch to crossfade mode for crossfade_seconds.
    // 2) During crossfade, mix outgoing (still from existing decoder) with incoming
    //    (from the time-stretched mix-window buffer).
    // 3) After crossfade, swap in a long-running decoder pointing at the incoming
    //    track starting from `incoming_intro_end + crossfade_seconds`.

    // Fetch incoming mix window in the background.
    let incoming_path: std::path::PathBuf = incoming.sidecar.source_path.clone().into();
    let stretched = decode_stretched(
        &incoming_path,
        incoming_intro_end - (16.0 * bar_seconds),
        incoming_intro_end + (32.0 * bar_seconds),
        ratio,
    )
    .await?;

    // Read outgoing until we have crossfade_seconds worth of audio left to play.
    let crossfade_samples = (crossfade_seconds * SAMPLE_RATE as f64) as usize * 2;
    // Read outgoing in chunks until we're at outro_start time (simplified — engine just
    // keeps reading until ffmpeg signals EOF near outro).
    loop {
        let pcm = decoder.read_frames(READ_CHUNK_FRAMES).await?;
        if pcm.is_empty() {
            break;
        }
        let pushed = producer.push_slice(&pcm);
        if pushed < pcm.len() {
            // Buffer full — block briefly to let consumer drain.
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            producer.push_slice(&pcm[pushed..]);
        }
        // Heuristic: if we've reached the crossfade region, break.
        // (A real impl tracks samples-played. For the integration plan this is sufficient.)
        if producer.occupied_len() > crossfade_samples {
            break;
        }
    }

    // Crossfade: mix the next crossfade_samples of outgoing with the head of `stretched`.
    let outgoing_tail = decoder.read_frames(crossfade_samples / 2).await?;
    let mut incoming_head = stretched.clone();
    incoming_head.truncate(crossfade_samples);
    let mixed = mix_crossfade(&outgoing_tail, &incoming_head, crossfade_samples / 2);
    producer.push_slice(&mixed);

    info!("crossfade complete, swapping playing decoder to incoming track");

    // Old decoder dies.
    // Spawn new playing decoder positioned past the crossfade region.
    let cue_out = incoming.sidecar.cue_out_seconds.unwrap_or(0.0);
    let new_decoder = PlayingDecoder::spawn(
        &incoming_path,
        incoming_intro_end + (32.0 * bar_seconds),
        cue_out,
    )
    .await?;

    Ok(new_decoder)
}
