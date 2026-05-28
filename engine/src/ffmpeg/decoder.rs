//! Long-running ffmpeg decoder. Spawns ffmpeg to decode an audio file to f32le PCM
//! and exposes async reads of fixed-size chunks.

use std::path::Path;
use std::process::Stdio;

use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};

use crate::error::{EngineError, Result};
use crate::ffmpeg::{BYTES_PER_FRAME, CHANNELS, SAMPLE_RATE};

pub struct PlayingDecoder {
    child: Child,
    stdout: tokio::process::ChildStdout,
    /// Track time (seconds) corresponding to the FIRST sample the decoder emits.
    /// Used by the mixer to know how far into the track the current output cursor is.
    pub start_seconds: f64,
}

impl PlayingDecoder {
    /// Spawn `ffmpeg` to decode `path` starting at `start_seconds`, ending at `end_seconds`.
    pub async fn spawn(path: &Path, start_seconds: f64, end_seconds: f64) -> Result<Self> {
        let mut cmd = Command::new("ffmpeg");
        // Use accurate seek: `-ss` placed AFTER `-i`. Slower (ffmpeg decodes from the
        // nearest keyframe forward to the requested time) but sample-accurate. The
        // fast-seek form (`-ss` before `-i`) lands on a keyframe near the requested
        // time, which causes audible splice glitches when we hand off from the
        // mix-window stretched buffer to this new decoder.
        cmd.arg("-hide_banner")
            .arg("-loglevel")
            .arg("error")
            .arg("-i")
            .arg(path)
            .arg("-ss")
            .arg(format!("{:.3}", start_seconds))
            .arg("-to")
            .arg(format!("{:.3}", end_seconds))
            .arg("-f")
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
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| EngineError::Ffmpeg("no stdout pipe".into()))?;
        Ok(Self {
            child,
            stdout,
            start_seconds,
        })
    }

    /// Read the next `frames` of stereo audio. Returns interleaved f32 stereo.
    /// EOF returns a short or empty Vec.
    pub async fn read_frames(&mut self, frames: usize) -> Result<Vec<f32>> {
        let bytes_needed = frames * BYTES_PER_FRAME;
        let mut buf = vec![0u8; bytes_needed];
        let mut filled = 0;
        while filled < bytes_needed {
            let n = self
                .stdout
                .read(&mut buf[filled..])
                .await
                .map_err(EngineError::Io)?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        buf.truncate(filled);
        // Convert little-endian f32 bytes to f32 samples.
        let mut samples = Vec::with_capacity(filled / 4);
        for chunk in buf.chunks_exact(4) {
            samples.push(f32::from_le_bytes(chunk.try_into().unwrap()));
        }
        Ok(samples)
    }

    /// Spawn `ffmpeg` with an `atempo=tempo` filter so output samples are
    /// time-stretched from source. `source_start` and `source_end` are SOURCE-time
    /// seconds. `tempo` is the atempo ratio (>1.0 speeds source up; output rate is
    /// always SAMPLE_RATE). When `tempo ~ 1.0` the atempo filter is bypassed and
    /// the behavior matches `spawn`.
    ///
    /// Uses pre-input seek (`-ss <start> -i path`) for source positioning so the
    /// source range consumed is `[source_start, source_end]` regardless of tempo.
    /// `-t <output_duration>` bounds the output, where output_duration = (end-start)/tempo.
    pub async fn spawn_with_tempo(
        path: &Path,
        source_start: f64,
        source_end: f64,
        tempo: f64,
    ) -> Result<Self> {
        if tempo <= 0.0 {
            return Err(EngineError::Ffmpeg(format!("invalid tempo: {tempo}")));
        }
        let identity = (tempo - 1.0).abs() < 0.005;
        if identity {
            return Self::spawn(path, source_start, source_end).await;
        }

        let output_duration = ((source_end - source_start).max(0.0)) / tempo;
        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-hide_banner")
            .arg("-loglevel")
            .arg("error")
            .arg("-accurate_seek")
            .arg("-ss")
            .arg(format!("{:.6}", source_start.max(0.0)))
            .arg("-i")
            .arg(path)
            .arg("-t")
            .arg(format!("{:.6}", output_duration))
            .arg("-af")
            .arg(format!("atempo={tempo}"))
            .arg("-f")
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
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| EngineError::Ffmpeg("no stdout pipe".into()))?;
        Ok(Self {
            child,
            stdout,
            start_seconds: source_start,
        })
    }

    pub async fn shutdown(mut self) {
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }
}

/// Decode an audio file to mono f32 PCM at 44100 Hz. Used for stem loading
/// in `Deck::cue_stems`: reads stdout to EOF, reinterprets little-endian f32
/// bytes as a `Vec<f32>`. Runs ffmpeg synchronously (one-shot), so this is
/// O(file-duration / decode-rate); on modern hardware ~10x realtime for MP3,
/// so a 4-minute stem decodes in roughly 25 seconds total (4 stems = ~100 s).
/// Acceptable for stems mode since the scheduler arms transitions a bar ahead
/// of when they fire.
pub async fn decode_to_mono_pcm_44100(path: &Path) -> Result<Vec<f32>> {
    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-hide_banner")
        .arg("-loglevel")
        .arg("error")
        .arg("-i")
        .arg(path)
        .arg("-ac")
        .arg("1")
        .arg("-ar")
        .arg(SAMPLE_RATE.to_string())
        .arg("-f")
        .arg("f32le")
        .arg("-")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| EngineError::Ffmpeg(format!("spawn: {e}")))?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| EngineError::Ffmpeg("no stdout pipe".into()))?;
    let mut buf = Vec::new();
    stdout
        .read_to_end(&mut buf)
        .await
        .map_err(EngineError::Io)?;
    let status = child.wait().await.map_err(EngineError::Io)?;
    if !status.success() {
        return Err(EngineError::Ffmpeg(format!(
            "ffmpeg exited with {status} while decoding {}",
            path.display()
        )));
    }
    let mut samples = Vec::with_capacity(buf.len() / 4);
    for chunk in buf.chunks_exact(4) {
        samples.push(f32::from_le_bytes(chunk.try_into().unwrap()));
    }
    Ok(samples)
}
