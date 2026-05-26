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
        return Err(EngineError::Ffmpeg(format!("invalid atempo ratio: {ratio}")));
    }
    let filter = atempo_chain(ratio);

    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-hide_banner")
        .arg("-loglevel").arg("error")
        .arg("-ss").arg(format!("{:.3}", start_seconds))
        .arg("-to").arg(format!("{:.3}", end_seconds))
        .arg("-i").arg(path)
        .arg("-af").arg(filter)
        .arg("-f").arg("f32le")
        .arg("-ar").arg(SAMPLE_RATE.to_string())
        .arg("-ac").arg(CHANNELS.to_string())
        .arg("-")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| EngineError::Ffmpeg(format!("spawn: {e}")))?;
    let mut stdout = child.stdout.take().ok_or_else(|| EngineError::Ffmpeg("no stdout".into()))?;
    let mut buf = Vec::new();
    stdout.read_to_end(&mut buf).await.map_err(EngineError::Io)?;

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
