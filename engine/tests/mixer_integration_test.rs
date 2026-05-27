use dj_engine::buffer::{make_ring, Consumer};
use dj_engine::deck::DeckId;
use dj_engine::ffmpeg::SAMPLE_RATE;
use dj_engine::library::LoadedTrack;
use dj_engine::mixer::{tick_until, Mixer};
use dj_engine::recipe::{BassSwap, EqualPowerCrossfade};
use dj_engine::scheduler::TransitionScheduler;
use dj_engine::sidecar::{BandValues, Sidecar};
use dj_engine::transition::TransitionPlan;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

fn track(name: &str, dur: f64) -> LoadedTrack {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/{name}"));
    LoadedTrack {
        sidecar_path: PathBuf::new(),
        sidecar: Sidecar {
            schema_version: 1,
            analyzer_version: "test".into(),
            source_path: path.to_string_lossy().into_owned(),
            source_mtime: 0.0,
            source_size_bytes: 0,
            duration_seconds: Some(dur),
            sample_rate: Some(44100),
            bpm: Some(120.0),
            bpm_confidence: Some(1.0),
            beats_seconds: vec![],
            downbeats_seconds: vec![],
            key: None,
            key_label: None,
            key_confidence: None,
            energy_curve: vec![],
            energy_mean: Some(0.5),
            band_energy_means: None,
            cue_in_seconds: Some(0.0),
            cue_out_seconds: Some(dur),
            intro_end_seconds: Some(0.0),
            outro_start_seconds: Some(dur),
            is_mixable: Some(true),
            mixable_reasons: vec![],
            error: None,
            phrase_boundaries_seconds: None,
            phrase_confidence: None,
            band_energy_curves: None,
            is_beatmatchable: None,
            is_phrase_aligned: None,
        },
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn full_transition_outputs_continuous_audio() {
    // Small ring (10s) — a concurrent drain task keeps push_all from blocking.
    let cap = (SAMPLE_RATE as usize) * 2 * 10;
    let (producer, mut consumer) = make_ring(cap);
    let mut mixer = Mixer::new(producer);

    let outgoing = track("sine_440_6s.wav", 6.0);
    let incoming = track("sine_660_6s.wav", 6.0);

    // Drain task: pulls samples out of the ring into a Vec so the producer is
    // never blocked. Stops when the `done` flag flips and the ring drains.
    let collected: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    let done: Arc<std::sync::atomic::AtomicBool> =
        Arc::new(std::sync::atomic::AtomicBool::new(false));
    let drain_collected = Arc::clone(&collected);
    let drain_done = Arc::clone(&done);
    let drain = tokio::spawn(async move {
        let mut buf = vec![0f32; 4096];
        loop {
            let n = consumer.pop_slice(&mut buf);
            if n > 0 {
                drain_collected.lock().unwrap().extend_from_slice(&buf[..n]);
            } else {
                if drain_done.load(std::sync::atomic::Ordering::SeqCst) {
                    // Final drain pass to catch anything still in the ring.
                    let n2 = consumer.pop_slice(&mut buf);
                    if n2 > 0 {
                        drain_collected
                            .lock()
                            .unwrap()
                            .extend_from_slice(&buf[..n2]);
                        continue;
                    }
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        }
    });

    // Cue + play outgoing on Deck A.
    {
        let d = mixer.deck(DeckId::A);
        d.cue(outgoing.clone(), 0.0, 1.0).await.unwrap();
        d.play();
        d.test_set_fader(1.0);
    }

    // Drain ~0.5 s of A solo so the consumer test can see audio before xfade.
    tick_until(&mut mixer, 22_050).await.unwrap();

    // 2-bar (~4s @ 120 BPM) crossfade fits inside both 6-second fixtures even
    // after the 0.5s pre-roll on Deck A. Same BPM -> no pitch-journey ramp.
    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 0.5,
        incoming_crossfade_start_seconds: 1.0,
        length_bars: 2,
        use_eq_swap: false,
    };
    let handle = TransitionScheduler::arm(
        &mut mixer,
        DeckId::A,
        DeckId::B,
        &outgoing,
        &incoming,
        &plan,
        &EqualPowerCrossfade,
    )
    .await
    .unwrap();
    tick_until(&mut mixer, handle.end_master_clock_frames)
        .await
        .unwrap();

    // Signal the drain task to wind down and wait for it.
    done.store(true, std::sync::atomic::Ordering::SeqCst);
    let _ = drain.await;

    let all = collected.lock().unwrap().clone();
    assert!(!all.is_empty());
    // Check for a long run of zeros (underrun).
    let mut zero_run = 0usize;
    let mut max_zero_run = 0usize;
    for s in &all {
        if *s == 0.0 {
            zero_run += 1;
            max_zero_run = max_zero_run.max(zero_run);
        } else {
            zero_run = 0;
        }
    }
    assert!(max_zero_run < 4096, "long silence: {max_zero_run} samples");
}

fn bass_heavy_track(name: &str, dur: f64) -> LoadedTrack {
    let mut t = track(name, dur);
    t.sidecar.band_energy_means = Some(BandValues {
        bass: 0.7,
        mid: 0.5,
        high: 0.5,
    });
    t
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bass_swap_transition_outputs_continuous_audio() {
    let cap = (SAMPLE_RATE as usize) * 2 * 10;
    let (producer, mut consumer) = make_ring(cap);
    let mut mixer = Mixer::new(producer);

    let outgoing = bass_heavy_track("sine_440_6s.wav", 6.0);
    let incoming = bass_heavy_track("sine_660_6s.wav", 6.0);

    // Drain task: same shape as the EqualPower integration test above.
    let collected: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    let done: Arc<std::sync::atomic::AtomicBool> =
        Arc::new(std::sync::atomic::AtomicBool::new(false));
    let drain_collected = Arc::clone(&collected);
    let drain_done = Arc::clone(&done);
    let drain = tokio::spawn(async move {
        let mut buf = vec![0f32; 4096];
        loop {
            let n = consumer.pop_slice(&mut buf);
            if n > 0 {
                drain_collected.lock().unwrap().extend_from_slice(&buf[..n]);
            } else {
                if drain_done.load(std::sync::atomic::Ordering::SeqCst) {
                    let n2 = consumer.pop_slice(&mut buf);
                    if n2 > 0 {
                        drain_collected
                            .lock()
                            .unwrap()
                            .extend_from_slice(&buf[..n2]);
                        continue;
                    }
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        }
    });

    {
        let d = mixer.deck(DeckId::A);
        d.cue(outgoing.clone(), 0.0, 1.0).await.unwrap();
        d.play();
        d.test_set_fader(1.0);
    }

    tick_until(&mut mixer, 22_050).await.unwrap();

    let plan = TransitionPlan {
        outgoing_crossfade_start_seconds: 0.5,
        incoming_crossfade_start_seconds: 1.0,
        length_bars: 2,
        use_eq_swap: true,
    };
    let handle = TransitionScheduler::arm(
        &mut mixer,
        DeckId::A,
        DeckId::B,
        &outgoing,
        &incoming,
        &plan,
        &BassSwap,
    )
    .await
    .unwrap();
    tick_until(&mut mixer, handle.end_master_clock_frames)
        .await
        .unwrap();

    done.store(true, std::sync::atomic::Ordering::SeqCst);
    let _ = drain.await;

    let all = collected.lock().unwrap().clone();
    assert!(!all.is_empty());
    let mut zero_run = 0usize;
    let mut max_zero_run = 0usize;
    for s in &all {
        if *s == 0.0 {
            zero_run += 1;
            max_zero_run = max_zero_run.max(zero_run);
        } else {
            zero_run = 0;
        }
    }
    assert!(max_zero_run < 4096, "long silence: {max_zero_run} samples");
}
