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
}

impl PlayingDecoder {
    /// Spawn `ffmpeg` to decode `path` starting at `start_seconds`, ending at `end_seconds`.
    pub async fn spawn(path: &Path, start_seconds: f64, end_seconds: f64) -> Result<Self> {
        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-hide_banner")
            .arg("-loglevel").arg("error")
            .arg("-ss").arg(format!("{:.3}", start_seconds))
            .arg("-to").arg(format!("{:.3}", end_seconds))
            .arg("-i").arg(path)
            .arg("-f").arg("f32le")
            .arg("-ar").arg(SAMPLE_RATE.to_string())
            .arg("-ac").arg(CHANNELS.to_string())
            .arg("-")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| EngineError::Ffmpeg(format!("spawn: {e}")))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| EngineError::Ffmpeg("no stdout pipe".into()))?;
        Ok(Self { child, stdout })
    }

    /// Read the next `frames` of stereo audio. Returns interleaved f32 stereo.
    /// EOF returns a short or empty Vec.
    pub async fn read_frames(&mut self, frames: usize) -> Result<Vec<f32>> {
        let bytes_needed = frames * BYTES_PER_FRAME;
        let mut buf = vec![0u8; bytes_needed];
        let mut filled = 0;
        while filled < bytes_needed {
            let n = self.stdout.read(&mut buf[filled..]).await
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

    pub async fn shutdown(mut self) {
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }
}
