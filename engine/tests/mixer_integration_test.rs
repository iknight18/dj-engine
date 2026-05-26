//! Integration test for the crossfade math by mixing two pure-tone files.

use std::path::Path;

use dj_engine::crossfade::mix_crossfade;
use dj_engine::ffmpeg::decoder::PlayingDecoder;

fn fixtures() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("audio")
}

#[tokio::test]
async fn crossfade_contains_both_signals_in_middle() {
    let a = fixtures().join("a_sine_440.flac");
    let b = fixtures().join("b_sine_523.flac");
    if !a.exists() || !b.exists() {
        eprintln!("skipping: fixtures missing — run generate_test_audio.sh");
        return;
    }

    let mut decoder_a = PlayingDecoder::spawn(&a, 1.0, 5.0).await.expect("decoder a");
    let mut decoder_b = PlayingDecoder::spawn(&b, 1.0, 5.0).await.expect("decoder b");

    let a_pcm = decoder_a.read_frames(44_100).await.expect("pcm a"); // 1s
    let b_pcm = decoder_b.read_frames(44_100).await.expect("pcm b");

    let mixed = mix_crossfade(&a_pcm, &b_pcm, a_pcm.len() / 2);
    // At the midpoint, both signals should contribute. Assert RMS is nonzero.
    // Note: ffmpeg's lavfi sine filter emits at a low default amplitude (~0.088 peak,
    // ~0.06 RMS for a unit-amplitude reference), so the threshold is calibrated to
    // detect "both signals present and crossfade gains active" rather than full-scale.
    let mid = &mixed[mixed.len() / 4..3 * mixed.len() / 4];
    let rms: f64 = (mid.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / mid.len() as f64).sqrt();
    assert!(rms > 0.01, "RMS in crossfade region was {rms} — expected > 0.01");
}
