//! Encoder supervisor — restarts the encoder ffmpeg subprocess on crash with
//! exponential backoff.

use std::time::Duration;

use tracing::{info, warn};

use crate::error::Result;
use crate::ffmpeg::encoder::{Encoder, EncoderConfig};

const INITIAL_DELAY_MS: u64 = 1_000;
const MAX_DELAY_MS: u64 = 60_000;

pub async fn spawn_with_retries(cfg: &EncoderConfig) -> Result<Encoder> {
    let mut delay_ms = INITIAL_DELAY_MS;
    loop {
        match Encoder::spawn(cfg).await {
            Ok(enc) => {
                info!("encoder spawned");
                return Ok(enc);
            }
            Err(e) => {
                warn!(error = %e, delay_ms, "encoder spawn failed, retrying");
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                delay_ms = (delay_ms * 2).min(MAX_DELAY_MS);
            }
        }
    }
}
