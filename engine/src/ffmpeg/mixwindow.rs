//! Short-lived ffmpeg subprocess that decodes a slice of a track AND applies
//! `atempo` time-stretching, returning the resulting PCM as a single buffer.

use std::path::Path;
use std::process::Stdio;

use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::error::{EngineError, Result};
use crate::ffmpeg::{CHANNELS, SAMPLE_RATE};

/// Decode `path` between `start_seconds` and `end_seconds`, time-stretched by `ratio`.
/// `ratio = current_bpm / candidate_bpm` (i.e., values >1 speed up the incoming track).
pub async fn decode_stretched(
    path: &Path,
    start_seconds: f64,
    end_seconds: f64,
    ratio: f64,
) -> Result<Vec<f32>> {
    if ratio <= 0.0 {
        return Err(EngineError::Ffmpeg(format!(
            "invalid atempo ratio: {ratio}"
        )));
    }

    // Bypass atempo when the ratio is essentially 1.0 — atempo (even at ratio=1.0)
    // adds ~50-100ms of algorithmic latency from its WSOLA windowing, which causes
    // a sample-discontinuity when handing off to the post-crossfade decoder.
    // Threshold: 0.5% drift (≈ unnoticeable BPM mismatch).
    let identity = (ratio - 1.0).abs() < 0.005;

    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-hide_banner").arg("-loglevel").arg("error");

    // Accurate seek (`-ss` after `-i`) to match the new decoder's seek semantics,
    // so the stretched buffer and the post-crossfade decoder line up sample-for-sample.
    cmd.arg("-i")
        .arg(path)
        .arg("-ss")
        .arg(format!("{:.3}", start_seconds))
        .arg("-to")
        .arg(format!("{:.3}", end_seconds));

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

/// Decode `output_seconds` of audio from `source_path` starting at source-time
/// `source_start_seconds`, time-stretched by `ratio`. Uses input seek (`-ss before -i`)
/// for source positioning and `-t output_seconds` to bound output duration — so the
/// source range consumed is `[source_start, source_start + output_seconds * ratio]`
/// and the returned buffer is exactly `output_seconds` of PCM at SAMPLE_RATE.
///
/// This is the source-positioned counterpart to `decode_stretched`: callers pass a
/// source-time start and an output duration, instead of source-time start/end.
pub async fn decode_atempo_at_source(
    source_path: &Path,
    source_start_seconds: f64,
    output_seconds: f64,
    ratio: f64,
) -> Result<Vec<f32>> {
    if ratio <= 0.0 {
        return Err(EngineError::Ffmpeg(format!("invalid atempo ratio: {ratio}")));
    }
    if output_seconds <= 0.0 {
        return Ok(Vec::new());
    }

    let identity = (ratio - 1.0).abs() < 0.005;

    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-hide_banner").arg("-loglevel").arg("error");
    // `-ss` BEFORE `-i` = input seek (positions the source); `-accurate_seek` is the
    // default but we set it explicitly so the source-time start is exact.
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

/// Empirically-measured WSOLA source-consumption overhead per `atempo` ffmpeg
/// invocation (MP3 source, ratios near 1.0). Each call consumes ~130ms more source
/// than `output * ratio` would predict. Used by both `decode_stretched_ramp` to
/// advance between chunks and by the mixer to position the new-decoder spawn point.
pub const ATEMPO_PER_CALL_OVERHEAD_S: f64 = 0.13;

/// Decode a ramp region of `source_path`, starting at source-time `source_start_seconds`,
/// producing `ramp_seconds` of stereo PCM output. The atempo ratio is linearly
/// interpolated between `start_ratio` and `end_ratio` across `chunks` equal-output-duration
/// chunks (midpoint interpolation: chunk `i` uses `lerp(start, end, (i + 0.5) / chunks)`).
///
/// Each chunk uses input-seek + `-t` so source positions are sample-precise: chunk `i+1`
/// begins where chunk `i`'s atempo invocation actually ended in source-time —
/// `src_pos += T * r_i + ATEMPO_PER_CALL_OVERHEAD_S` (the overhead compensates for
/// atempo's WSOLA window consuming a bit more source than the per-ratio math implies).
/// Total source consumed is `ramp_seconds * (start + end) / 2 + chunks * overhead`.
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
        let chunk = decode_atempo_at_source(source_path, src_pos, chunk_out_seconds, ratio).await?;
        out.extend_from_slice(&chunk);
        src_pos += chunk_out_seconds * ratio + ATEMPO_PER_CALL_OVERHEAD_S;
    }
    Ok(out)
}

/// Source-time consumed by a ramp with the given parameters — `chunks` atempo calls,
/// each with output `ramp_seconds/chunks` at midpoint-interpolated ratio. Returns the
/// sum of (T * r_i + overhead) over all chunks.
pub fn ramp_source_consumed_seconds(
    start_ratio: f64,
    end_ratio: f64,
    ramp_seconds: f64,
    chunks: usize,
) -> f64 {
    ramp_seconds * (start_ratio + end_ratio) / 2.0
        + (chunks as f64) * ATEMPO_PER_CALL_OVERHEAD_S
}

/// Build an atempo filter chain — atempo only accepts ratios in [0.5, 2.0], so for
/// ratios outside that we chain multiple atempo instances (rarely needed; we cap at ±10%).
fn atempo_chain(ratio: f64) -> String {
    if (0.5..=2.0).contains(&ratio) {
        return format!("atempo={ratio}");
    }
    // Decompose into factors within [0.5, 2.0].
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
