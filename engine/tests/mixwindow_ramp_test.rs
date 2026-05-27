//! Tests for decode_stretched_ramp — chunked atempo with a per-chunk ratio ramp.

use std::path::Path;

use dj_engine::ffmpeg::mixwindow::decode_stretched_ramp;
use dj_engine::ffmpeg::SAMPLE_RATE;

fn fixtures() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("audio")
}

#[tokio::test]
async fn identity_ratio_produces_expected_sample_count() {
    let a = fixtures().join("a_sine_440.flac");
    if !a.exists() {
        eprintln!("skipping: fixtures missing — run generate_test_audio.sh");
        return;
    }

    let pcm = decode_stretched_ramp(&a, 0.0, 1.0, 1.0, 2.0, 4)
        .await
        .expect("decode");

    let expected = (2.0 * SAMPLE_RATE as f64) as usize * 2;
    let diff = (pcm.len() as i64 - expected as i64).abs();
    assert!(
        diff < (16 * 2) as i64,
        "got {} samples, expected ~{} (diff {})",
        pcm.len(),
        expected,
        diff
    );
}

#[tokio::test]
async fn ratio_ramp_consumes_expected_input_seconds() {
    let a = fixtures().join("a_sine_440.flac");
    if !a.exists() {
        eprintln!("skipping: fixtures missing");
        return;
    }

    let pcm = decode_stretched_ramp(&a, 0.0, 1.05, 1.0, 2.0, 4)
        .await
        .expect("decode");

    let expected_out = (2.0 * SAMPLE_RATE as f64) as usize * 2;
    let diff = (pcm.len() as i64 - expected_out as i64).abs();
    assert!(
        diff < (16 * 2 * 4) as i64,
        "got {} samples, expected ~{} (diff {})",
        pcm.len(),
        expected_out,
        diff
    );
}

#[tokio::test]
async fn zero_chunks_returns_empty() {
    let a = fixtures().join("a_sine_440.flac");
    let pcm = decode_stretched_ramp(&a, 0.0, 1.05, 1.0, 2.0, 0)
        .await
        .expect("decode");
    assert_eq!(pcm.len(), 0);
}

#[tokio::test]
async fn zero_ramp_seconds_returns_empty() {
    let a = fixtures().join("a_sine_440.flac");
    let pcm = decode_stretched_ramp(&a, 0.0, 1.05, 1.0, 0.0, 4)
        .await
        .expect("decode");
    assert_eq!(pcm.len(), 0);
}
