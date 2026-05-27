//! ffmpeg subprocess orchestration. Two roles:
//! - `decoder`: long-running PCM streamer for the currently playing track
//! - `encoder`: long-running encoder that pushes to Icecast
//!
//! (The former `mixwindow` short-lived stretched-decode helpers now live in a
//! private `crate::deck::stretch` module.)

pub mod decoder;
pub mod encoder;

pub const SAMPLE_RATE: u32 = 44100;
pub const CHANNELS: u32 = 2;
pub const SAMPLE_BYTES: usize = 4; // f32le stereo = 8 bytes/frame, 4 bytes/sample
pub const BYTES_PER_FRAME: usize = (CHANNELS as usize) * SAMPLE_BYTES;
