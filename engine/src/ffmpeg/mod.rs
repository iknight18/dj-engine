//! ffmpeg subprocess orchestration. Three roles:
//! - `decoder`: long-running PCM streamer for the currently playing track
//! - `mixwindow`: short-lived decoder + atempo for the upcoming track's mix window
//! - `encoder`: long-running encoder that pushes to Icecast

pub mod decoder;
pub mod encoder;
pub mod mixwindow;

pub const SAMPLE_RATE: u32 = 44100;
pub const CHANNELS: u32 = 2;
pub const SAMPLE_BYTES: usize = 4; // f32le stereo = 8 bytes/frame, 4 bytes/sample
pub const BYTES_PER_FRAME: usize = (CHANNELS as usize) * SAMPLE_BYTES;
