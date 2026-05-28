use dj_engine::effects::{EchoLine, LoopBuffer};

const EPS: f32 = 1e-6;

#[test]
fn echo_line_zero_delay_returns_silence() {
    let mut e = EchoLine::new(100);
    e.set_params(0, 0.5);
    let (l, r) = e.process(1.0, 1.0);
    assert!((l).abs() < EPS);
    assert!((r).abs() < EPS);
}

#[test]
fn echo_line_decaying_taps_at_delay_intervals() {
    // delay=4 samples, feedback=0.5. Push an impulse, then read 20 samples and
    // confirm decaying taps at offsets 4, 8, 12, 16 with amplitudes 1.0, 0.5, 0.25, 0.125.
    let mut e = EchoLine::new(100);
    e.set_params(4, 0.5);

    let (_l, _r) = e.process(1.0, 1.0);
    let mut left_samples = Vec::new();
    for _ in 0..20 {
        let (l, _r) = e.process(0.0, 0.0);
        left_samples.push(l);
    }

    // Expected taps at indices 3, 7, 11, 15 (since the first process consumed the
    // impulse and stored it; the next process reads delay=4 back).
    let expected = [(3, 1.0), (7, 0.5), (11, 0.25), (15, 0.125)];
    for (idx, amp) in expected {
        assert!(
            (left_samples[idx] - amp).abs() < 1e-3,
            "tap at index {idx} should be {amp}, got {}",
            left_samples[idx]
        );
    }
    // Confirm non-tap positions inside the first 4 tap windows are near zero.
    // (Beyond index 18 the IIR feedback keeps producing further decaying taps;
    // those are valid behavior and not checked here.)
    for (i, s) in left_samples.iter().enumerate().take(18) {
        if ![3, 7, 11, 15].contains(&i) {
            assert!(s.abs() < 1e-3, "non-tap at {i}: {s}");
        }
    }
}

#[test]
fn echo_line_clear_zeroes_state() {
    let mut e = EchoLine::new(100);
    e.set_params(4, 0.5);
    e.process(1.0, 1.0);
    e.process(1.0, 1.0);
    e.clear();
    // After clear, processing zero input should yield zero output.
    for _ in 0..20 {
        let (l, r) = e.process(0.0, 0.0);
        assert!(l.abs() < EPS);
        assert!(r.abs() < EPS);
    }
}

#[test]
fn echo_line_set_params_preserves_buffer() {
    // Push energy, then change params; subsequent zero-input reads should still
    // produce non-zero output (buffer contents survived).
    let mut e = EchoLine::new(100);
    e.set_params(4, 0.5);
    for _ in 0..8 {
        e.process(1.0, 1.0);
    }
    e.set_params(8, 0.5);
    let (l, _r) = e.process(0.0, 0.0);
    // Some non-zero output expected (buffer has prior energy).
    assert!(
        l.abs() > 1e-3 || {
            // Try a few more reads
            let mut any_nonzero = false;
            for _ in 0..15 {
                let (l2, _) = e.process(0.0, 0.0);
                if l2.abs() > 1e-3 {
                    any_nonzero = true;
                    break;
                }
            }
            any_nonzero
        }
    );
}

#[test]
fn loop_buffer_write_lock_read_replays_input() {
    let mut b = LoopBuffer::new(20);
    b.set_loop_samples(10);
    // Write a known ramp.
    for i in 0..10 {
        b.write(i as f32, (i + 100) as f32);
    }
    b.lock();
    // Read 10 samples — should match the written ramp in order.
    for i in 0..10 {
        let (l, r) = b.read();
        assert!((l - i as f32).abs() < EPS, "l at {i}: got {l}");
        assert!((r - (i + 100) as f32).abs() < EPS, "r at {i}: got {r}");
    }
    // Next read should wrap back to the start of the loop region.
    let (l, _r) = b.read();
    assert!((l - 0.0).abs() < EPS, "expected wrap to 0.0, got {l}");
}

#[test]
fn loop_buffer_unlock_then_relock_uses_fresh_capture() {
    let mut b = LoopBuffer::new(20);
    b.set_loop_samples(10);
    for i in 0..10 {
        b.write(i as f32, 0.0);
    }
    b.lock();
    b.read();
    b.unlock();
    // Write new audio.
    for i in 100..110 {
        b.write(i as f32, 0.0);
    }
    b.lock();
    // The locked region should now reflect the new writes (100..109).
    let mut firsts: Vec<f32> = (0..10).map(|_| b.read().0).collect();
    firsts.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert!(
        firsts[0] >= 100.0 - EPS,
        "expected fresh writes, lowest was {}",
        firsts[0]
    );
}

#[test]
fn loop_buffer_clear_zeros_state() {
    let mut b = LoopBuffer::new(20);
    b.set_loop_samples(10);
    for i in 0..10 {
        b.write(i as f32, 0.0);
    }
    b.clear();
    assert!(!b.is_locked());
    // Lock and read — should be all zeros (capture was cleared).
    b.lock();
    for _ in 0..10 {
        let (l, _r) = b.read();
        assert!(l.abs() < EPS);
    }
}
