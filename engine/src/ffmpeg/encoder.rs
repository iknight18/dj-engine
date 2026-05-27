//! Long-running ffmpeg subprocess that reads f32le PCM on stdin, encodes (MP3 or Opus),
//! and pushes to an Icecast source mount.

use std::process::Stdio;

use tokio::io::AsyncWriteExt;
use tokio::process::{Child, ChildStdin, Command};

use crate::error::{EngineError, Result};
use crate::ffmpeg::{CHANNELS, SAMPLE_RATE};

#[derive(Debug, Clone)]
pub struct EncoderConfig {
    pub icecast_url: String,
    pub user: String,
    pub password: String,
    pub format: EncoderFormat,
    pub bitrate: String, // e.g., "192k"
}

#[derive(Debug, Clone, Copy)]
pub enum EncoderFormat {
    Mp3,
    Opus,
}

pub struct Encoder {
    pub child: Child,
    pub stdin: ChildStdin,
}

impl Encoder {
    pub async fn spawn(cfg: &EncoderConfig) -> Result<Self> {
        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-hide_banner")
            .arg("-loglevel")
            .arg("info")
            .arg("-re")
            .arg("-f")
            .arg("f32le")
            .arg("-ar")
            .arg(SAMPLE_RATE.to_string())
            .arg("-ac")
            .arg(CHANNELS.to_string())
            .arg("-i")
            .arg("pipe:0");

        match cfg.format {
            EncoderFormat::Mp3 => {
                cmd.arg("-c:a")
                    .arg("libmp3lame")
                    .arg("-b:a")
                    .arg(&cfg.bitrate)
                    .arg("-content_type")
                    .arg("audio/mpeg")
                    .arg("-f")
                    .arg("mp3");
            }
            EncoderFormat::Opus => {
                cmd.arg("-c:a")
                    .arg("libopus")
                    .arg("-b:a")
                    .arg(&cfg.bitrate)
                    .arg("-content_type")
                    .arg("audio/ogg")
                    .arg("-f")
                    .arg("ogg");
            }
        }

        let url = build_icecast_url(&cfg.icecast_url, &cfg.user, &cfg.password);
        cmd.arg(url)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());

        let mut child = cmd
            .spawn()
            .map_err(|e| EngineError::Ffmpeg(format!("spawn: {e}")))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| EngineError::Ffmpeg("no stdin".into()))?;
        Ok(Self { child, stdin })
    }

    pub async fn write_pcm(&mut self, samples: &[f32]) -> Result<()> {
        let mut bytes = Vec::with_capacity(samples.len() * 4);
        for s in samples {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        self.stdin
            .write_all(&bytes)
            .await
            .map_err(EngineError::Io)?;
        Ok(())
    }
}

fn build_icecast_url(url: &str, user: &str, password: &str) -> String {
    // url is expected to look like icecast://host:port/mount; inject user:password.
    if let Some(rest) = url.strip_prefix("icecast://") {
        let user_enc = urlencoding::encode(user);
        let pw_enc = urlencoding::encode(password);
        format!("icecast://{user_enc}:{pw_enc}@{rest}")
    } else {
        url.to_string()
    }
}
